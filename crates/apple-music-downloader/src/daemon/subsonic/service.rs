use apple_music_api::{Artwork, SearchRequest, SongPlaybackMetadata};
use apple_music_decryptor::{ArtworkDescriptor, PlaybackTrackMetadata};

use super::data::{
    album_detail_to_subsonic, artwork_template, render_artwork_url, search_results,
    search_song_to_subsonic, string_at,
};
use super::error::{SubsonicError, map_app_error};
use super::{DaemonContext, ResponseFormat};

pub(super) async fn load_song(
    context: &DaemonContext,
    song_id: &str,
    format: ResponseFormat,
) -> Result<super::data::SubsonicSong, SubsonicError> {
    let session = context
        .session()
        .map_err(|error| map_app_error(format, error))?;
    let profile = session.account_profile();
    let song = context
        .api
        .song(
            context.default_storefront(),
            context.default_language(),
            &profile.dev_token,
            song_id,
        )
        .await
        .map_err(|error| map_app_error(format, error.into()))?;
    let item = search_results(&song, "/data").first().ok_or_else(|| {
        SubsonicError::not_found(format, format!("song {song_id} did not return any data"))
    })?;
    Ok(search_song_to_subsonic(item))
}

pub(super) async fn load_lyrics(
    context: &DaemonContext,
    song_id: &str,
    format: ResponseFormat,
) -> Result<String, SubsonicError> {
    let session = context
        .session()
        .map_err(|error| map_app_error(format, error))?;
    let profile = session.account_profile();
    context
        .api
        .lyrics(
            context.default_storefront(),
            context.default_language(),
            &profile.music_token,
            song_id,
        )
        .await
        .map_err(|error| map_app_error(format, error.into()))
}

pub(super) async fn load_lyrics_optional(context: &DaemonContext, song_id: &str) -> Option<String> {
    let session = context.session().ok()?;
    let profile = session.account_profile();
    context
        .api
        .lyrics(
            context.default_storefront(),
            context.default_language(),
            &profile.music_token,
            song_id,
        )
        .await
        .ok()
}

pub(super) async fn resolve_lyrics_song_id(
    context: &DaemonContext,
    artist: Option<&str>,
    title: Option<&str>,
    format: ResponseFormat,
) -> Result<String, SubsonicError> {
    let title = title
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SubsonicError::required_parameter(format, "title is required"))?;
    let search = context
        .api
        .search(SearchRequest {
            storefront: context.default_storefront(),
            language: context.default_language(),
            query: title,
            search_type: "song",
            limit: 20,
            offset: 0,
        })
        .await
        .map_err(|error| map_app_error(format, error.into()))?;
    let requested_artist = artist.map(normalize_match_text);
    let requested_title = normalize_match_text(title);
    let songs = search_results(&search, "/results/songs/data");
    let exact = songs.iter().find(|item| {
        normalize_match_text(string_at(item, "/attributes/name").unwrap_or_default())
            == requested_title
            && requested_artist.as_ref().is_none_or(|artist| {
                normalize_match_text(string_at(item, "/attributes/artistName").unwrap_or_default())
                    == *artist
            })
    });
    let fallback = songs.iter().find(|item| {
        normalize_match_text(string_at(item, "/attributes/name").unwrap_or_default())
            == requested_title
    });
    exact
        .or(fallback)
        .and_then(|item| string_at(item, "/id"))
        .map(str::to_owned)
        .ok_or_else(|| SubsonicError::not_found(format, "lyrics target song not found"))
}

pub(super) async fn resolve_artwork(
    context: &DaemonContext,
    dev_token: &str,
    item_id: &str,
    size: Option<u32>,
    format: ResponseFormat,
) -> Result<Option<ArtworkDescriptor>, SubsonicError> {
    if let Ok(song) = context
        .api
        .song(
            context.default_storefront(),
            context.default_language(),
            dev_token,
            item_id,
        )
        .await
    {
        if let Some(template) = artwork_template(&song, "/data/0/attributes/artwork/url") {
            return Ok(Some(ArtworkDescriptor {
                url: render_artwork_url(template, size),
                width: size,
                height: size,
            }));
        }
        if let Some(template) = artwork_template(
            &song,
            "/data/0/relationships/albums/data/0/attributes/artwork/url",
        ) {
            return Ok(Some(ArtworkDescriptor {
                url: render_artwork_url(template, size),
                width: size,
                height: size,
            }));
        }
    }

    if let Ok(album) = context
        .api
        .album(
            context.default_storefront(),
            context.default_language(),
            dev_token,
            item_id,
        )
        .await
        && let Some(template) = artwork_template(&album, "/data/0/attributes/artwork/url")
    {
        return Ok(Some(ArtworkDescriptor {
            url: render_artwork_url(template, size),
            width: size,
            height: size,
        }));
    }

    if let Ok(artist) = context
        .api
        .artist(
            context.default_storefront(),
            context.default_language(),
            dev_token,
            item_id,
            Some("latest-release"),
            Some(1),
        )
        .await
    {
        if let Some(template) = artwork_template(&artist, "/data/0/attributes/artwork/url") {
            return Ok(Some(ArtworkDescriptor {
                url: render_artwork_url(template, size),
                width: size,
                height: size,
            }));
        }
        if let Some(template) = artwork_template(
            &artist,
            "/data/0/views/latest-release/data/0/attributes/artwork/url",
        ) {
            return Ok(Some(ArtworkDescriptor {
                url: render_artwork_url(template, size),
                width: size,
                height: size,
            }));
        }
    }

    let _ = format;
    Ok(None)
}

pub(super) fn requested_codec(
    max_bit_rate: Option<u32>,
    format: Option<&str>,
    response_format: ResponseFormat,
) -> Result<Option<String>, SubsonicError> {
    let format = format.unwrap_or("").trim().to_ascii_lowercase();
    let codec = match format.as_str() {
        "" | "raw" | "m4a" | "mp4" => {
            if max_bit_rate.is_some() {
                Some("aac".to_owned())
            } else {
                None
            }
        }
        "aac" => Some("aac".to_owned()),
        "alac" | "lossless" => Some("alac".to_owned()),
        other => {
            return Err(SubsonicError::required_parameter(
                response_format,
                format!("unsupported stream format: {other}"),
            ));
        }
    };
    Ok(codec)
}

pub(super) fn playback_track_metadata(
    metadata: SongPlaybackMetadata,
    lyrics: Option<String>,
) -> PlaybackTrackMetadata {
    PlaybackTrackMetadata {
        song_id: metadata.song_id,
        artist: metadata.artist,
        artist_id: metadata.artist_id,
        album_id: metadata.album_id,
        album: metadata.album,
        title: metadata.title,
        track_number: metadata.track_number,
        disc_number: metadata.disc_number,
        artwork: metadata.artwork.map(playback_artwork),
        album_artwork: metadata.album_artwork.map(playback_artwork),
        lyrics,
    }
}

fn playback_artwork(artwork: Artwork) -> ArtworkDescriptor {
    ArtworkDescriptor {
        url: render_artwork_url(&artwork.url, None),
        width: artwork.width,
        height: artwork.height,
    }
}

pub(super) fn normalize_match_text(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

pub(super) fn album_summary_with_songs(
    album: &serde_json::Value,
) -> Option<(super::data::SubsonicAlbum, Vec<super::data::SubsonicSong>)> {
    let mut album_summary = album_detail_to_subsonic(album)?;
    let songs = search_results(album, "/data/0/relationships/tracks/data")
        .iter()
        .map(super::data::album_track_to_subsonic)
        .collect::<Vec<_>>();
    album_summary.duration = Some(songs.iter().filter_map(|song| song.duration).sum());
    album_summary.song_count = Some(songs.len());
    Some((album_summary, songs))
}
