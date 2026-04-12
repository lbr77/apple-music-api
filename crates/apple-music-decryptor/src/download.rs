use std::collections::HashMap;
use std::fs::{self, File};
use std::io;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::LazyLock;

use mp4ameta::{Img, Tag};
use regex::Regex;
use reqwest::Proxy;
use reqwest::blocking::{Client, ClientBuilder};
use reqwest::header::RANGE;
use serde::Serialize;
use serde_json::Value;

use crate::config::DownloadConfig;
use crate::error::{AppResult, AppleMusicDecryptorError as AppError};
use crate::ffi::ContextKey;
use crate::session::SessionRuntime;

use super::mp4;

const DESKTOP_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36";
pub(crate) const FFMPEG_BINARY: &str = "/usr/local/bin/ffmpeg";
pub(crate) const FFPROBE_BINARY: &str = "/usr/local/bin/ffprobe";

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
pub struct BinaryHealth {
    pub path: &'static str,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ToolHealthReport {
    pub ffmpeg: BinaryHealth,
    pub ffprobe: BinaryHealth,
}

impl ToolHealthReport {
    pub fn is_healthy(&self) -> bool {
        self.ffmpeg.available && self.ffprobe.available
    }
}

#[derive(Clone, Debug)]
pub struct ArtworkDescriptor {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct PlaybackTrackMetadata {
    pub song_id: String,
    pub artist: String,
    pub artist_id: String,
    pub album_id: String,
    pub album: String,
    pub title: String,
    pub track_number: u32,
    pub disc_number: u32,
    pub artwork: Option<ArtworkDescriptor>,
    pub album_artwork: Option<ArtworkDescriptor>,
    pub lyrics: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PlaybackRequest {
    pub metadata: PlaybackTrackMetadata,
    pub requested_codec: Option<String>,
}

pub fn tool_health_report() -> ToolHealthReport {
    ToolHealthReport {
        ffmpeg: inspect_binary(FFMPEG_BINARY),
        ffprobe: inspect_binary(FFPROBE_BINARY),
    }
}

pub fn download_playback(
    config: DownloadConfig,
    session: std::sync::Arc<SessionRuntime>,
    request: PlaybackRequest,
) -> AppResult<PlaybackOutput> {
    let track = request.metadata;
    let song_id = track.song_id.clone();
    let client = build_client(&config)?;
    let album_dir = config.cache_dir.join("albums").join(&track.album_id);
    fs::create_dir_all(&album_dir)?;
    let final_path = album_dir.join(format!("{song_id}.m4a"));
    let relative_path = format!("cache/albums/{}/{}.m4a", track.album_id, song_id);

    if final_path.is_file() {
        return Ok(PlaybackOutput {
            relative_path,
            size: final_path.metadata()?.len(),
            artist: track.artist,
            artist_id: track.artist_id,
            album_id: track.album_id,
            album: track.album,
            title: track.title,
            codec: detect_codec_label(&final_path)?,
        });
    }

    let master_url = session.resolve_m3u8_url(song_id.parse::<u64>().map_err(|error| {
        AppError::Protocol(format!("song id is not a valid adam id: {error}"))
    })?)?;
    let master_text = client.get(&master_url).send()?.error_for_status()?.text()?;
    let variants = parse_master_playlist(&master_url, &master_text)?;
    let variant = choose_variant(&variants, request.requested_codec.as_deref())?;
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
    embed_mp4_tags(&client, &track, &final_temp_path)?;
    fs::rename(&final_temp_path, &final_path)?;

    for path in [&fragmented_path, &aac_path, &init_probe_path] {
        if path.is_file() {
            let _ = fs::remove_file(path);
        }
    }

    Ok(PlaybackOutput {
        relative_path,
        size: final_path.metadata()?.len(),
        artist: track.artist,
        artist_id: track.artist_id,
        album_id: track.album_id,
        album: track.album,
        title: track.title,
        codec: variant.codec_label(),
    })
}

fn build_client(config: &DownloadConfig) -> AppResult<Client> {
    let mut builder = ClientBuilder::new().user_agent(DESKTOP_USER_AGENT);
    if let Some(proxy) = config.proxy.as_deref() {
        builder = builder.proxy(Proxy::all(proxy)?);
    }
    Ok(builder.build()?)
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
    let args = [
        "-v".into(),
        "error".into(),
        "-show_streams".into(),
        "-of".into(),
        "json".into(),
        path.to_string_lossy().into_owned(),
    ];
    let output = run_binary("probe AAC stream", FFPROBE_BINARY, &args)?;
    if !output.status.success() {
        return Err(command_failure_error(
            "probe AAC stream",
            FFPROBE_BINARY,
            &args,
            &output,
        ));
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
    // Keep audio remux aligned with the upstream downloader: audio-only output is assembled
    // once and then tagged in place instead of carrying fragmented MP4 metadata forward.
    // multi-track assembly.
    let input = if is_aac_variant {
        aac_path
    } else {
        fragmented_path
    };
    let args = ffmpeg_remux_args(input, final_path);
    let output = run_binary("remux playback", FFMPEG_BINARY, &args)?;
    if !output.status.success() {
        return Err(command_failure_error(
            "remux playback",
            FFMPEG_BINARY,
            &args,
            &output,
        ));
    }
    Ok(())
}

fn embed_mp4_tags(
    client: &Client,
    track: &PlaybackTrackMetadata,
    final_path: &Path,
) -> AppResult<()> {
    let mut tag = Tag::read_from_path(final_path)?;
    tag.set_artist(track.artist.clone());
    tag.set_album_artist(track.artist.clone());
    tag.set_title(track.title.clone());
    tag.set_album(track.album.clone());
    tag.set_track_number(u16::try_from(track.track_number).map_err(|error| {
        AppError::Protocol(format!(
            "track number does not fit in mp4 tag field: {error}"
        ))
    })?);
    tag.set_disc_number(u16::try_from(track.disc_number).map_err(|error| {
        AppError::Protocol(format!(
            "disc number does not fit in mp4 tag field: {error}"
        ))
    })?);

    if let Some(lyrics) = track.lyrics.as_deref().filter(|lyrics| !lyrics.is_empty()) {
        tag.set_lyrics(lyrics.to_owned());
    }

    if let Some(artwork) = download_cover_artwork(client, track)? {
        tag.set_artwork(artwork);
    }

    tag.write_to_path(final_path)?;
    Ok(())
}

fn download_cover_artwork(
    client: &Client,
    track: &PlaybackTrackMetadata,
) -> AppResult<Option<Img<Vec<u8>>>> {
    let artwork = track.artwork.as_ref().or(track.album_artwork.as_ref());
    let Some(artwork) = artwork else {
        return Ok(None);
    };

    let response = client
        .get(artwork_url(artwork))
        .send()?
        .error_for_status()?;
    let bytes = response.bytes()?.to_vec();
    let image = if is_jpeg(&bytes) {
        Img::jpeg(bytes)
    } else if is_png(&bytes) {
        Img::png(bytes)
    } else if is_bmp(&bytes) {
        Img::bmp(bytes)
    } else {
        return Err(AppError::Protocol(
            "downloaded artwork is not a supported JPEG/PNG/BMP image".into(),
        ));
    };
    Ok(Some(image))
}

fn artwork_url(artwork: &ArtworkDescriptor) -> String {
    let width = artwork.width.unwrap_or(1200);
    let height = artwork.height.unwrap_or(width);
    artwork
        .url
        .replace("{w}", &width.to_string())
        .replace("{h}", &height.to_string())
}

fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'])
}

fn is_bmp(bytes: &[u8]) -> bool {
    bytes.starts_with(b"BM")
}

fn detect_codec_label(path: &Path) -> AppResult<String> {
    let args = [
        "-v".into(),
        "error".into(),
        "-show_streams".into(),
        "-of".into(),
        "json".into(),
        path.to_string_lossy().into_owned(),
    ];
    let output = run_binary("detect output codec", FFPROBE_BINARY, &args)?;
    if !output.status.success() {
        return Err(command_failure_error(
            "detect output codec",
            FFPROBE_BINARY,
            &args,
            &output,
        ));
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

/// Include the command line in backend errors so missing runtime tools can be located
/// from the HTTP response without shell access to the server.
fn run_binary(stage: &'static str, path: &'static str, args: &[String]) -> AppResult<Output> {
    Command::new(path)
        .args(args)
        .output()
        .map_err(|error| command_spawn_error(stage, path, args, &error))
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

fn command_spawn_error(
    stage: &'static str,
    path: &'static str,
    args: &[String],
    error: &io::Error,
) -> AppError {
    AppError::Command(format!(
        "{stage} failed to spawn {}: {error}",
        command_invocation(path, args),
    ))
}

fn command_failure_error(
    stage: &'static str,
    path: &'static str,
    args: &[String],
    output: &Output,
) -> AppError {
    AppError::Command(format!(
        "{stage} failed: {}: {}",
        command_invocation(path, args),
        command_output_message(output),
    ))
}

fn command_invocation(path: &str, args: &[String]) -> String {
    if args.is_empty() {
        return path.to_owned();
    }
    let rendered_args = args
        .iter()
        .map(|arg| format!("{arg:?}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{path} {rendered_args}")
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
