use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::LazyLock;

use regex::Regex;
use reqwest::Proxy;
use reqwest::blocking::{Client, ClientBuilder};
use reqwest::header::{AUTHORIZATION, ORIGIN, RANGE, REFERER};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::ffi::ContextKey;
use crate::runtime::SessionRuntime;

use super::mp4;

const MUSIC_ORIGIN: &str = "https://music.apple.com";
const DESKTOP_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36";
pub(crate) const FFMPEG_BINARY: &str = "/usr/local/bin/ffmpeg";
pub(crate) const FFPROBE_BINARY: &str = "/usr/local/bin/ffprobe";
pub(crate) const MP4BOX_BINARY: &str = "/usr/local/bin/MP4Box";

static ATTR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"([A-Z0-9-]+)=(".*?"|[^,]+)"#).expect("valid attribute regex"));
static SANITIZE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[^A-Za-z0-9._-]+"#).expect("valid filename regex"));

#[derive(Debug, Serialize)]
pub struct PlaybackOutput {
    pub relative_path: String,
    pub size: u64,
    pub artist: String,
    pub artist_id: String,
    pub album_id: String,
    pub album: String,
    pub title: String,
    pub codec: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct BinaryHealth {
    pub path: &'static str,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ToolHealthReport {
    pub ffmpeg: BinaryHealth,
    pub ffprobe: BinaryHealth,
    pub mp4box: BinaryHealth,
}

impl ToolHealthReport {
    pub(crate) fn is_healthy(&self) -> bool {
        self.ffmpeg.available && self.ffprobe.available
    }
}

pub(crate) fn tool_health_report() -> ToolHealthReport {
    ToolHealthReport {
        ffmpeg: inspect_binary(FFMPEG_BINARY),
        ffprobe: inspect_binary(FFPROBE_BINARY),
        mp4box: inspect_binary(MP4BOX_BINARY),
    }
}

pub fn download_playback(
    config: AppConfig,
    session: std::sync::Arc<SessionRuntime>,
    storefront: String,
    language: Option<String>,
    song_id: String,
    requested_codec: Option<String>,
) -> AppResult<PlaybackOutput> {
    let profile = session.account_profile();
    let client = build_client(&config)?;
    let song = fetch_song(
        &client,
        &storefront,
        language.as_deref(),
        &profile.dev_token,
        &song_id,
    )?;
    let song_data = song
        .data
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Message(format!("song {song_id} did not return any data")))?;
    let album = song_data
        .relationships
        .albums
        .data
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Message(format!("song {song_id} is missing album metadata")))?;
    let artist = song_data
        .relationships
        .artists
        .data
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Message(format!("song {song_id} is missing artist metadata")))?;

    let album_dir = config.cache_dir.join("albums").join(&album.id);
    fs::create_dir_all(&album_dir)?;
    let final_path = album_dir.join(format!("{song_id}.m4a"));
    let relative_path = format!("cache/albums/{}/{}.m4a", album.id, song_id);

    if final_path.is_file() {
        return Ok(PlaybackOutput {
            relative_path,
            size: final_path.metadata()?.len(),
            artist: song_data.attributes.artist_name,
            artist_id: artist.id,
            album_id: album.id,
            album: song_data.attributes.album_name,
            title: song_data.attributes.name,
            codec: detect_codec_label(&final_path)?,
        });
    }

    let master_url = session.resolve_m3u8_url(song_id.parse::<u64>().map_err(|error| {
        AppError::Protocol(format!("song id is not a valid adam id: {error}"))
    })?)?;
    let master_text = client.get(&master_url).send()?.error_for_status()?.text()?;
    let variants = parse_master_playlist(&master_url, &master_text)?;
    let variant = choose_variant(&variants, requested_codec.as_deref())?;
    let media_text = client
        .get(&variant.uri)
        .send()?
        .error_for_status()?
        .text()?;
    let playlist = parse_media_playlist(&variant.uri, &media_text)?;

    let init_data = download_range(
        &client,
        &playlist.init.uri,
        playlist.init.offset,
        playlist.init.length,
    )?;
    if init_data.len() != playlist.init.length {
        return Err(AppError::Message(
            "downloaded init segment length does not match playlist byterange".into(),
        ));
    }
    let init_data = mp4::sanitize_init_segment(&init_data)?;

    let variant_stem = sanitized_variant_stem(&variant.uri);
    let fragmented_path = album_dir.join(format!("{song_id}_{variant_stem}.frag.m4a"));
    let final_temp_path = album_dir.join(format!("{song_id}_{variant_stem}.m4a"));
    let aac_path = album_dir.join(format!("{song_id}_{variant_stem}.aac"));
    let init_probe_path = album_dir.join(format!("{song_id}_{variant_stem}.init.mp4"));
    fs::write(&init_probe_path, &init_data)?;

    let is_aac_variant = variant.codecs.to_ascii_lowercase().contains("mp4a");
    let (aac_sample_rate, aac_channels) = if is_aac_variant {
        probe_aac_stream(&init_probe_path)?
    } else {
        (0, 0)
    };

    let mut fragmented = File::create(&fragmented_path)?;
    fragmented.write_all(&init_data)?;
    let mut aac_output = is_aac_variant
        .then(|| File::create(&aac_path))
        .transpose()?;

    let native = session.native();
    let mut contexts = HashMap::new();

    for segment in playlist.segments {
        let fragment = download_range(&client, &segment.uri, segment.offset, segment.length)?;
        if fragment.len() != segment.length {
            return Err(AppError::Message(format!(
                "downloaded {} bytes for segment {}, expected {}",
                fragment.len(),
                segment.index,
                segment.length,
            )));
        }

        let sample_slices = mp4::collect_sample_slices(&fragment)?;
        let context_key = segment.key_uri.clone();
        if !contexts.contains_key(&context_key) {
            let context = native.build_context(&ContextKey {
                adam: song_id.clone(),
                uri: context_key.clone(),
            })?;
            contexts.insert(context_key.clone(), context);
        }
        let context = contexts
            .get_mut(&context_key)
            .ok_or_else(|| AppError::Message("decrypt context was not retained".into()))?;

        let mut fragment_out = fragment.clone();
        for sample_slice in sample_slices {
            let sample = &fragment[sample_slice.clone()];
            let decrypted = decrypt_sample(native.as_ref(), context, sample)?;
            if decrypted.len() != sample.len() {
                return Err(AppError::Message("decrypt sample length mismatch".into()));
            }
            if let Some(aac_output) = aac_output.as_mut() {
                aac_output.write_all(&mp4::make_adts_header(
                    decrypted.len(),
                    aac_sample_rate,
                    aac_channels,
                )?)?;
                aac_output.write_all(&decrypted)?;
            }
            fragment_out[sample_slice].copy_from_slice(&decrypted);
        }

        let sanitized = mp4::sanitize_fragment(&fragment_out)?;
        fragmented.write_all(&sanitized)?;
    }

    if let Some(mut file) = aac_output {
        file.flush()?;
    }
    fragmented.flush()?;

    remux_output(
        is_aac_variant,
        &fragmented_path,
        &aac_path,
        &final_temp_path,
    )?;
    fs::rename(&final_temp_path, &final_path)?;

    for path in [&fragmented_path, &aac_path, &init_probe_path] {
        if path.is_file() {
            let _ = fs::remove_file(path);
        }
    }

    Ok(PlaybackOutput {
        relative_path,
        size: final_path.metadata()?.len(),
        artist: song_data.attributes.artist_name,
        artist_id: artist.id,
        album_id: album.id,
        album: song_data.attributes.album_name,
        title: song_data.attributes.name,
        codec: variant.codec_label(),
    })
}

fn build_client(config: &AppConfig) -> AppResult<Client> {
    let mut builder = ClientBuilder::new().user_agent(DESKTOP_USER_AGENT);
    if let Some(proxy) = config.proxy.as_deref() {
        builder = builder.proxy(Proxy::all(proxy)?);
    }
    Ok(builder.build()?)
}

fn fetch_song(
    client: &Client,
    storefront: &str,
    language: Option<&str>,
    dev_token: &str,
    song_id: &str,
) -> AppResult<SongResponse> {
    let mut request = client
        .get(format!(
            "https://amp-api.music.apple.com/v1/catalog/{storefront}/songs/{song_id}"
        ))
        .header(AUTHORIZATION, format!("Bearer {dev_token}"))
        .header(ORIGIN, MUSIC_ORIGIN)
        .header(REFERER, format!("{MUSIC_ORIGIN}/"))
        .query(&[
            ("include", "albums,artists"),
            ("extend", "extendedAssetUrls"),
        ]);
    if let Some(language) = language.filter(|value| !value.is_empty()) {
        request = request.query(&[("l", language)]);
    }
    Ok(request.send()?.error_for_status()?.json()?)
}

fn parse_attrs(text: &str) -> HashMap<String, String> {
    ATTR_RE
        .captures_iter(text)
        .map(|capture| {
            (
                capture[1].to_owned(),
                capture[2].trim_matches('"').to_owned(),
            )
        })
        .collect()
}

fn parse_master_playlist(base_url: &str, text: &str) -> AppResult<Vec<Variant>> {
    let mut variants = Vec::new();
    let mut pending = None::<HashMap<String, String>>;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            pending = Some(parse_attrs(rest));
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let Some(attrs) = pending.take() else {
            continue;
        };
        variants.push(Variant {
            uri: resolve_url(base_url, line)?,
            average_bandwidth: attrs
                .get("AVERAGE-BANDWIDTH")
                .or_else(|| attrs.get("BANDWIDTH"))
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0),
            bandwidth: attrs
                .get("BANDWIDTH")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0),
            codecs: attrs.get("CODECS").cloned().unwrap_or_default(),
        });
    }

    if variants.is_empty() {
        return Err(AppError::Message(
            "master playlist did not contain any variants".into(),
        ));
    }
    Ok(variants)
}

fn choose_variant<'a>(
    variants: &'a [Variant],
    requested_codec: Option<&str>,
) -> AppResult<&'a Variant> {
    let requested = requested_codec
        .unwrap_or("alac")
        .trim()
        .to_ascii_lowercase();
    let candidate = match requested.as_str() {
        "alac" => variants
            .iter()
            .filter(|variant| variant.codecs.to_ascii_lowercase().contains("alac"))
            .max_by_key(|variant| (variant.average_bandwidth, variant.bandwidth)),
        "aac" => variants
            .iter()
            .filter(|variant| variant.codecs.to_ascii_lowercase().contains("mp4a"))
            .max_by_key(|variant| (variant.average_bandwidth, variant.bandwidth)),
        "auto" | "" => variants
            .iter()
            .max_by_key(|variant| (variant.average_bandwidth, variant.bandwidth)),
        other => {
            return Err(AppError::Protocol(format!(
                "unsupported codec selection: {other}"
            )));
        }
    };

    candidate
        .or_else(|| {
            variants
                .iter()
                .max_by_key(|variant| (variant.average_bandwidth, variant.bandwidth))
        })
        .ok_or_else(|| AppError::Message("failed to choose a playlist variant".into()))
}

fn parse_media_playlist(base_url: &str, text: &str) -> AppResult<MediaPlaylist> {
    let mut current_key_uri = None::<String>;
    let mut pending_duration = None::<f32>;
    let mut pending_length = None::<usize>;
    let mut pending_offset = None::<usize>;
    let mut next_offset = None::<usize>;
    let mut init = None::<ByteRangeSegment>;
    let mut segments = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-KEY:") {
            let attrs = parse_attrs(rest);
            current_key_uri = attrs.get("URI").cloned();
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-MAP:") {
            let attrs = parse_attrs(rest);
            let byterange = attrs.get("BYTERANGE").ok_or_else(|| {
                AppError::Protocol("media playlist init map omitted BYTERANGE".into())
            })?;
            let (length, offset) = parse_byterange(byterange, None)?;
            next_offset = Some(offset + length);
            init = Some(ByteRangeSegment {
                uri: resolve_url(
                    base_url,
                    attrs.get("URI").ok_or_else(|| {
                        AppError::Protocol("media playlist init map omitted URI".into())
                    })?,
                )?,
                length,
                offset,
            });
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            pending_duration = Some(
                rest.split(',')
                    .next()
                    .ok_or_else(|| AppError::Protocol("invalid EXTINF line".into()))?
                    .parse::<f32>()
                    .map_err(|error| {
                        AppError::Protocol(format!("invalid segment duration: {error}"))
                    })?,
            );
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            let previous_end = next_offset;
            let (length, offset) = parse_byterange(rest, previous_end)?;
            pending_length = Some(length);
            pending_offset = Some(offset);
            next_offset = Some(offset + length);
            continue;
        }
        if line.starts_with('#') {
            continue;
        }

        let key_uri = current_key_uri
            .clone()
            .ok_or_else(|| AppError::Protocol("segment appeared before any EXT-X-KEY".into()))?;
        let length = pending_length.take().ok_or_else(|| {
            AppError::Protocol("segment appeared before byte-range length".into())
        })?;
        let offset = pending_offset.take().ok_or_else(|| {
            AppError::Protocol("segment appeared before byte-range offset".into())
        })?;
        let duration = pending_duration
            .take()
            .ok_or_else(|| AppError::Protocol("segment appeared before duration".into()))?;

        segments.push(MediaSegment {
            index: segments.len(),
            uri: resolve_url(base_url, line)?,
            length,
            offset,
            duration,
            key_uri,
        });
    }

    let init =
        init.ok_or_else(|| AppError::Protocol("media playlist did not include EXT-X-MAP".into()))?;
    if segments.is_empty() {
        return Err(AppError::Protocol(
            "media playlist did not include any media segments".into(),
        ));
    }
    Ok(MediaPlaylist { init, segments })
}

fn parse_byterange(value: &str, fallback_offset: Option<usize>) -> AppResult<(usize, usize)> {
    let (length_text, offset_text) = value
        .split_once('@')
        .map_or((value, None), |(a, b)| (a, Some(b)));
    let length = length_text
        .trim()
        .parse::<usize>()
        .map_err(|error| AppError::Protocol(format!("invalid byte-range length: {error}")))?;
    let offset = match offset_text {
        Some(offset) => offset
            .trim()
            .parse::<usize>()
            .map_err(|error| AppError::Protocol(format!("invalid byte-range offset: {error}")))?,
        None => fallback_offset.ok_or_else(|| {
            AppError::Protocol("segment byterange omitted offset before any previous range".into())
        })?,
    };
    Ok((length, offset))
}

fn resolve_url(base_url: &str, value: &str) -> AppResult<String> {
    Ok(reqwest::Url::parse(base_url)
        .map_err(|error| AppError::Protocol(format!("invalid playlist base url: {error}")))?
        .join(value)
        .map_err(|error| AppError::Protocol(format!("invalid playlist uri: {error}")))?
        .to_string())
}

fn download_range(client: &Client, url: &str, offset: usize, length: usize) -> AppResult<Vec<u8>> {
    let end = offset + length - 1;
    let response = client
        .get(url)
        .header(RANGE, format!("bytes={offset}-{end}"))
        .send()?
        .error_for_status()?;
    Ok(response.bytes()?.to_vec())
}

fn decrypt_sample(
    native: &crate::ffi::NativeSession,
    context: &mut crate::ffi::PContextHandle,
    sample: &[u8],
) -> AppResult<Vec<u8>> {
    let truncated = sample.len() & !0x0F;
    if truncated == 0 {
        return Ok(sample.to_vec());
    }
    let mut decrypted = native.decrypt_sample(context, sample[..truncated].to_vec())?;
    decrypted.extend_from_slice(&sample[truncated..]);
    Ok(decrypted)
}

fn sanitized_variant_stem(variant_uri: &str) -> String {
    let path = reqwest::Url::parse(variant_uri)
        .ok()
        .and_then(|url| {
            Path::new(url.path())
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "track".into());
    let sanitized = SANITIZE_RE.replace_all(&path, "_");
    sanitized.trim_matches('.').trim_matches('_').to_owned()
}

fn probe_aac_stream(path: &Path) -> AppResult<(u32, u8)> {
    let output = run_binary(
        FFPROBE_BINARY,
        &[
            "-v".into(),
            "error".into(),
            "-show_streams".into(),
            "-of".into(),
            "json".into(),
            path.to_string_lossy().into_owned(),
        ],
    )?;
    if !output.status.success() {
        return Err(AppError::Message(command_output_message(&output)));
    }
    let json: Value = serde_json::from_slice(&output.stdout)?;
    let stream = json
        .get("streams")
        .and_then(Value::as_array)
        .and_then(|streams| {
            streams.iter().find(|stream| {
                stream
                    .get("codec_type")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == "audio")
            })
        })
        .ok_or_else(|| AppError::Message("ffprobe did not return an audio stream".into()))?;
    let sample_rate = stream
        .get("sample_rate")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Message("ffprobe audio stream omitted sample_rate".into()))?
        .parse::<u32>()
        .map_err(|error| AppError::Protocol(format!("invalid AAC sample rate: {error}")))?;
    let channels = stream
        .get("channels")
        .and_then(Value::as_u64)
        .ok_or_else(|| AppError::Message("ffprobe audio stream omitted channels".into()))?
        as u8;
    Ok((sample_rate, channels))
}

fn remux_output(
    is_aac_variant: bool,
    fragmented_path: &Path,
    aac_path: &Path,
    final_path: &Path,
) -> AppResult<()> {
    // GPAC remuxes sanitized non-AAC fragmented MP4 more reliably than a blind ffmpeg copy,
    // so prefer MP4Box when it is installed at the expected runtime path.
    let strategy = choose_remux_strategy(is_aac_variant, Path::new(MP4BOX_BINARY).is_file());
    let output = match strategy {
        RemuxStrategy::Ffmpeg => {
            let input = if is_aac_variant {
                aac_path
            } else {
                fragmented_path
            };
            run_binary(FFMPEG_BINARY, &ffmpeg_remux_args(input, final_path))?
        }
        RemuxStrategy::Mp4Box => run_binary(
            MP4BOX_BINARY,
            &mp4box_remux_args(fragmented_path, final_path),
        )?,
    };
    if !output.status.success() {
        return Err(AppError::Message(command_output_message(&output)));
    }
    Ok(())
}

fn detect_codec_label(path: &Path) -> AppResult<String> {
    let output = run_binary(
        FFPROBE_BINARY,
        &[
            "-v".into(),
            "error".into(),
            "-show_streams".into(),
            "-of".into(),
            "json".into(),
            path.to_string_lossy().into_owned(),
        ],
    )?;
    if !output.status.success() {
        return Err(AppError::Message(command_output_message(&output)));
    }
    let json: Value = serde_json::from_slice(&output.stdout)?;
    let codec = json
        .get("streams")
        .and_then(Value::as_array)
        .and_then(|streams| {
            streams.iter().find_map(|stream| {
                (stream.get("codec_type").and_then(Value::as_str) == Some("audio"))
                    .then(|| stream.get("codec_name").and_then(Value::as_str))
                    .flatten()
            })
        })
        .unwrap_or("unknown");
    Ok(match codec {
        "alac" => "ALAC",
        "aac" => "AAC",
        other => other,
    }
    .to_owned())
}

fn inspect_binary(path: &'static str) -> BinaryHealth {
    match Command::new(path).arg("-version").output() {
        Ok(output) if output.status.success() => BinaryHealth {
            path,
            available: true,
            version: String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned),
            error: None,
        },
        Ok(output) => BinaryHealth {
            path,
            available: false,
            version: None,
            error: Some(command_output_message(&output)),
        },
        Err(error) => BinaryHealth {
            path,
            available: false,
            version: None,
            error: Some(error.to_string()),
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RemuxStrategy {
    Ffmpeg,
    Mp4Box,
}

fn choose_remux_strategy(is_aac_variant: bool, mp4box_available: bool) -> RemuxStrategy {
    if is_aac_variant || !mp4box_available {
        RemuxStrategy::Ffmpeg
    } else {
        RemuxStrategy::Mp4Box
    }
}

fn ffmpeg_remux_args(input: &Path, output: &Path) -> Vec<String> {
    vec![
        "-y".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        input.to_string_lossy().into_owned(),
        "-c".into(),
        "copy".into(),
        output.to_string_lossy().into_owned(),
    ]
}

fn mp4box_remux_args(input: &Path, output: &Path) -> Vec<String> {
    vec![
        "-quiet".into(),
        "-add".into(),
        input.to_string_lossy().into_owned(),
        "-new".into(),
        output.to_string_lossy().into_owned(),
    ]
}

fn run_binary(path: &'static str, args: &[String]) -> AppResult<Output> {
    Ok(Command::new(path).args(args).output()?)
}

fn command_output_message(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .chain(String::from_utf8_lossy(&output.stdout).lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("process exited with status {}", output.status))
}

#[derive(Debug)]
struct Variant {
    uri: String,
    average_bandwidth: u64,
    bandwidth: u64,
    codecs: String,
}

impl Variant {
    fn codec_label(&self) -> String {
        let codecs = self.codecs.to_ascii_lowercase();
        if codecs.contains("alac") {
            "ALAC".into()
        } else if codecs.contains("mp4a") {
            "AAC".into()
        } else {
            self.codecs.clone()
        }
    }
}

#[derive(Debug)]
struct ByteRangeSegment {
    uri: String,
    length: usize,
    offset: usize,
}

#[derive(Debug)]
struct MediaSegment {
    index: usize,
    uri: String,
    length: usize,
    offset: usize,
    #[allow(dead_code)]
    duration: f32,
    key_uri: String,
}

#[derive(Debug)]
struct MediaPlaylist {
    init: ByteRangeSegment,
    segments: Vec<MediaSegment>,
}

#[derive(Debug, Deserialize)]
struct SongResponse {
    data: Vec<SongData>,
}

#[derive(Debug, Deserialize)]
struct SongData {
    attributes: SongAttributes,
    relationships: SongRelationships,
}

#[derive(Debug, Deserialize)]
struct SongAttributes {
    #[serde(rename = "artistName")]
    artist_name: String,
    #[serde(rename = "albumName")]
    album_name: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct SongRelationships {
    artists: Relationship<ArtistData>,
    albums: Relationship<AlbumData>,
}

#[derive(Debug, Deserialize)]
struct Relationship<T> {
    data: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct ArtistData {
    id: String,
}

#[derive(Debug, Deserialize)]
struct AlbumData {
    id: String,
}
