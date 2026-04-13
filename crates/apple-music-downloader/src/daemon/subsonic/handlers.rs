use std::sync::Arc;

use apple_music_api::SearchRequest;
use axum::extract::{Query, Request, State};
use axum::http::header::{CONTENT_TYPE, HeaderValue};
use axum::response::{IntoResponse, Response};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use super::auth::response_format;
use super::data::{
    SubsonicArtist, SubsonicLyrics, search_album_to_subsonic, search_artist_to_subsonic,
    search_results, search_song_to_subsonic, unique_artist_albums,
};
use super::error::{SubsonicError, map_app_error};
use super::render::{
    album_json, album_xml, artist_json, artist_xml, escape_xml_attr, escape_xml_text, song_json,
    song_xml, subsonic_ok_json, subsonic_ok_response, subsonic_ok_xml,
};
use super::service::{
    album_summary_with_songs, load_lyrics, load_lyrics_optional, load_song, requested_codec,
    resolve_artwork, resolve_lyrics_song_id,
};
use super::{
    AuthQuery, CoverArtQuery, DaemonContext, IdQuery, LyricsQuery, ResponseFormat,
    SUBSONIC_MUSIC_FOLDER_ID, Search3Query, StreamQuery,
};

const APPLE_SEARCH_LIMIT_MAX: usize = 50;

pub(super) async fn log_subsonic_request(
    request: Request,
    next: axum::middleware::Next,
) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let query = request.uri().query().unwrap_or("").to_owned();
    crate::app_info!(
        "http::subsonic",
        "incoming subsonic request: method={}, path={}, query={}",
        method,
        path,
        query,
    );
    let response = next.run(request).await;
    crate::app_info!(
        "http::subsonic",
        "completed subsonic request: method={}, path={}, status={}",
        method,
        path,
        response.status(),
    );
    response
}

pub(super) async fn ping_handler(
    Query(query): Query<AuthQuery>,
) -> Result<Response, SubsonicError> {
    Ok(subsonic_ok_response(response_format(&query)?, None, ""))
}

pub(super) async fn get_license_handler(
    Query(query): Query<AuthQuery>,
) -> Result<Response, SubsonicError> {
    let format = response_format(&query)?;
    Ok(match format {
        ResponseFormat::Json => subsonic_ok_json(serde_json::json!({
            "license": { "valid": true }
        })),
        ResponseFormat::Xml => subsonic_ok_xml(r#"<license valid="true"/>"#),
    })
}

pub(super) async fn get_music_folders_handler(
    Query(query): Query<AuthQuery>,
) -> Result<Response, SubsonicError> {
    let format = response_format(&query)?;
    Ok(match format {
        ResponseFormat::Json => subsonic_ok_json(serde_json::json!({
            "musicFolders": {
                "musicFolder": [
                    {
                        "id": SUBSONIC_MUSIC_FOLDER_ID,
                        "name": "Apple Music"
                    }
                ]
            }
        })),
        ResponseFormat::Xml => subsonic_ok_xml(&format!(
            r#"<musicFolders><musicFolder id="{SUBSONIC_MUSIC_FOLDER_ID}" name="Apple Music"/></musicFolders>"#
        )),
    })
}

pub(super) async fn get_artists_handler(
    Query(query): Query<AuthQuery>,
) -> Result<Response, SubsonicError> {
    Err(SubsonicError::generic(
        response_format(&query)?,
        "Apple Music catalog does not expose server-side artist enumeration; use search3 instead",
    ))
}

pub(super) async fn get_indexes_handler(
    Query(query): Query<AuthQuery>,
) -> Result<Response, SubsonicError> {
    Err(SubsonicError::generic(
        response_format(&query)?,
        "Apple Music catalog does not expose server-side artist enumeration; use search3 instead",
    ))
}

pub(super) async fn search3_handler(
    State(context): State<Arc<DaemonContext>>,
    Query(query): Query<Search3Query>,
) -> Result<Response, SubsonicError> {
    let format = response_format(&query.auth)?;
    if query.query.trim().is_empty() {
        return Err(SubsonicError::required_parameter(
            format,
            "query is required",
        ));
    }
    let requested_artist_count = query.artist_count.unwrap_or(20);
    let requested_album_count = query.album_count.unwrap_or(20);
    let requested_song_count = query.song_count.unwrap_or(20);
    let artist_count = clamp_search_limit(requested_artist_count);
    let album_count = clamp_search_limit(requested_album_count);
    let song_count = clamp_search_limit(requested_song_count);

    crate::app_info!(
        "http::subsonic",
        "processing search3 request: query_len={}, requested_artist_count={}, requested_album_count={}, requested_song_count={}, artist_count={}, album_count={}, song_count={}",
        query.query.trim().len(),
        requested_artist_count,
        requested_album_count,
        requested_song_count,
        artist_count,
        album_count,
        song_count,
    );
    let storefront = context.default_storefront();
    let artist_hits = context
        .api
        .search(SearchRequest {
            storefront,
            language: context.default_language(),
            query: &query.query,
            search_type: "artist",
            limit: artist_count,
            offset: query.artist_offset.unwrap_or(0),
        })
        .await
        .map_err(|error| map_app_error(format, error.into()))?;
    let album_hits = context
        .api
        .search(SearchRequest {
            storefront,
            language: context.default_language(),
            query: &query.query,
            search_type: "album",
            limit: album_count,
            offset: query.album_offset.unwrap_or(0),
        })
        .await
        .map_err(|error| map_app_error(format, error.into()))?;
    let song_hits = context
        .api
        .search(SearchRequest {
            storefront,
            language: context.default_language(),
            query: &query.query,
            search_type: "song",
            limit: song_count,
            offset: query.song_offset.unwrap_or(0),
        })
        .await
        .map_err(|error| map_app_error(format, error.into()))?;

    let artists = search_results(&artist_hits, "/results/artists/data")
        .iter()
        .map(search_artist_to_subsonic)
        .collect::<Vec<_>>();
    let albums = search_results(&album_hits, "/results/albums/data")
        .iter()
        .map(search_album_to_subsonic)
        .collect::<Vec<_>>();
    let songs = search_results(&song_hits, "/results/songs/data")
        .iter()
        .map(search_song_to_subsonic)
        .collect::<Vec<_>>();

    Ok(match format {
        ResponseFormat::Json => subsonic_ok_json(serde_json::json!({
            "searchResult3": {
                "artist": artists.iter().map(artist_json).collect::<Vec<_>>(),
                "album": albums.iter().map(album_json).collect::<Vec<_>>(),
                "song": songs.iter().map(song_json).collect::<Vec<_>>(),
            }
        })),
        ResponseFormat::Xml => {
            let artists_xml = artists.iter().map(artist_xml).collect::<String>();
            let albums_xml = albums.iter().map(album_xml).collect::<String>();
            let songs_xml = songs.iter().map(song_xml).collect::<String>();
            subsonic_ok_xml(&format!(
                "<searchResult3>{artists_xml}{albums_xml}{songs_xml}</searchResult3>"
            ))
        }
    })
}

pub(super) fn clamp_search_limit(requested: usize) -> usize {
    requested.clamp(1, APPLE_SEARCH_LIMIT_MAX)
}

pub(super) async fn get_artist_handler(
    State(context): State<Arc<DaemonContext>>,
    Query(query): Query<IdQuery>,
) -> Result<Response, SubsonicError> {
    let format = response_format(&query.auth)?;
    let session = context
        .session()
        .map_err(|error| map_app_error(format, error))?;
    let profile = session.account_profile();
    let artist = context
        .api
        .artist(
            context.default_storefront(),
            context.default_language(),
            &profile.dev_token,
            &query.id,
            Some("full-albums,singles,latest-release"),
            // Apple rejects large artist limits for this view set on some catalogs.
            None,
        )
        .await
        .map_err(|error| map_app_error(format, error.into()))?;

    let (artist_name, artist_image_url, albums) = unique_artist_albums(&artist);
    let payload_artist = SubsonicArtist {
        id: query.id.clone(),
        name: artist_name,
        cover_art: super::data::artwork_template(&artist, "/data/0/attributes/artwork/url")
            .map(|_| query.id.clone()),
        artist_image_url,
        album_count: Some(albums.len()),
    };
    Ok(match format {
        ResponseFormat::Json => {
            let mut artist_value = artist_json(&payload_artist)
                .as_object()
                .expect("artist json object")
                .clone();
            artist_value.insert(
                "album".into(),
                serde_json::Value::Array(albums.iter().map(album_json).collect::<Vec<_>>()),
            );
            subsonic_ok_json(serde_json::json!({
                "artist": serde_json::Value::Object(artist_value)
            }))
        }
        ResponseFormat::Xml => {
            let album_xml = albums.iter().map(album_xml).collect::<String>();
            subsonic_ok_xml(&format!(
                "<artist id=\"{}\" name=\"{}\"{}{}>{album_xml}</artist>",
                escape_xml_attr(&payload_artist.id),
                escape_xml_attr(&payload_artist.name),
                payload_artist
                    .album_count
                    .map(|count| format!(r#" albumCount="{count}""#))
                    .unwrap_or_default(),
                payload_artist
                    .artist_image_url
                    .as_deref()
                    .map(|value| format!(r#" artistImageUrl="{}""#, escape_xml_attr(value)))
                    .unwrap_or_default(),
            ))
        }
    })
}

pub(super) async fn get_album_handler(
    State(context): State<Arc<DaemonContext>>,
    Query(query): Query<IdQuery>,
) -> Result<Response, SubsonicError> {
    let format = response_format(&query.auth)?;
    let session = context
        .session()
        .map_err(|error| map_app_error(format, error))?;
    let profile = session.account_profile();
    let album = context
        .api
        .album(
            context.default_storefront(),
            context.default_language(),
            &profile.dev_token,
            &query.id,
        )
        .await
        .map_err(|error| map_app_error(format, error.into()))?;

    let Some((album_summary, songs)) = album_summary_with_songs(&album) else {
        return Err(SubsonicError::not_found(
            format,
            "album did not return any data",
        ));
    };

    Ok(match format {
        ResponseFormat::Json => {
            let mut album_value = album_json(&album_summary)
                .as_object()
                .expect("album json object")
                .clone();
            album_value.insert(
                "song".into(),
                serde_json::Value::Array(songs.iter().map(song_json).collect::<Vec<_>>()),
            );
            subsonic_ok_json(serde_json::json!({
                "album": serde_json::Value::Object(album_value)
            }))
        }
        ResponseFormat::Xml => {
            let songs_xml = songs.iter().map(song_xml).collect::<String>();
            subsonic_ok_xml(&format!(
                "<album{}>{songs_xml}</album>",
                album_xml(&album_summary)
                    .trim_start_matches("<album")
                    .trim_end_matches("/>")
            ))
        }
    })
}

pub(super) async fn get_song_handler(
    State(context): State<Arc<DaemonContext>>,
    Query(query): Query<IdQuery>,
) -> Result<Response, SubsonicError> {
    let format = response_format(&query.auth)?;
    let song = load_song(&context, &query.id, format).await?;
    Ok(match format {
        ResponseFormat::Json => subsonic_ok_json(serde_json::json!({
            "song": song_json(&song)
        })),
        ResponseFormat::Xml => subsonic_ok_xml(&song_xml(&song)),
    })
}

pub(super) async fn get_lyrics_handler(
    State(context): State<Arc<DaemonContext>>,
    Query(query): Query<LyricsQuery>,
) -> Result<Response, SubsonicError> {
    let format = response_format(&query.auth)?;
    let song_id = if let Some(song_id) = query.id.as_deref().filter(|value| !value.is_empty()) {
        song_id.to_owned()
    } else {
        resolve_lyrics_song_id(
            &context,
            query.artist.as_deref(),
            query.title.as_deref(),
            format,
        )
        .await?
    };
    let song = load_song(&context, &song_id, format).await?;
    let lyrics_value = load_lyrics(&context, &song_id, format).await?;
    let lyrics = SubsonicLyrics {
        artist: song.artist.clone(),
        title: song.title.clone(),
        value: lyrics_value,
    };
    Ok(match format {
        ResponseFormat::Json => subsonic_ok_json(serde_json::json!({
            "lyrics": {
                "artist": lyrics.artist,
                "title": lyrics.title,
                "value": lyrics.value,
            }
        })),
        ResponseFormat::Xml => subsonic_ok_xml(&format!(
            "<lyrics artist=\"{}\" title=\"{}\">{}</lyrics>",
            escape_xml_attr(&lyrics.artist),
            escape_xml_attr(&lyrics.title),
            escape_xml_text(&lyrics.value),
        )),
    })
}

pub(super) async fn get_cover_art_handler(
    State(context): State<Arc<DaemonContext>>,
    Query(query): Query<CoverArtQuery>,
) -> Result<Response, SubsonicError> {
    let format = response_format(&query.auth)?;
    let session = context
        .session()
        .map_err(|error| map_app_error(format, error))?;
    let profile = session.account_profile();
    let artwork =
        match resolve_artwork(&context, &profile.dev_token, &query.id, query.size, format).await? {
            Some(artwork) => artwork,
            None => return Err(SubsonicError::not_found(format, "cover art not found")),
        };
    let mut client_builder = reqwest::Client::builder();
    if let Some(proxy) = context.config.proxy.as_deref() {
        client_builder = client_builder.proxy(
            reqwest::Proxy::all(proxy)
                .map_err(|error| SubsonicError::generic(format, error.to_string()))?,
        );
    }
    let client = client_builder
        .build()
        .map_err(|error| SubsonicError::generic(format, error.to_string()))?;
    let response = client
        .get(&artwork.url)
        .send()
        .await
        .map_err(|error| SubsonicError::generic(format, error.to_string()))?;
    if !response.status().is_success() {
        return Err(SubsonicError::not_found(
            format,
            format!("cover art request failed: {}", response.status()),
        ));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_owned();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| SubsonicError::generic(format, error.to_string()))?;

    Ok((
        [(
            CONTENT_TYPE,
            HeaderValue::from_str(&content_type).unwrap_or(HeaderValue::from_static("image/jpeg")),
        )],
        bytes,
    )
        .into_response())
}

pub(super) async fn stream_handler(
    State(context): State<Arc<DaemonContext>>,
    Query(query): Query<StreamQuery>,
    request: Request,
) -> Result<Response, SubsonicError> {
    let format = response_format(&query.auth)?;
    let session = context
        .session()
        .map_err(|error| map_app_error(format, error))?;
    let profile = session.account_profile();
    crate::app_info!(
        "http::subsonic",
        "stream request: song_id={}, max_bit_rate={}, format={}",
        query.id,
        query.max_bit_rate.unwrap_or_default(),
        query.format.as_deref().unwrap_or(""),
    );
    let metadata = context
        .api
        .song_playback_metadata(
            context.default_storefront(),
            context.default_language(),
            &profile.dev_token,
            &query.id,
        )
        .await
        .map_err(|error| map_app_error(format, error.into()))?;
    let lyrics = load_lyrics_optional(&context, &query.id).await;
    let selected_codec = requested_codec(query.max_bit_rate, query.format.as_deref(), format)?;
    let config = context.config.download_config();
    let playback = tokio::task::spawn_blocking(move || {
        apple_music_decryptor::download_playback(
            config,
            session,
            apple_music_decryptor::PlaybackRequest {
                metadata: super::service::playback_track_metadata(metadata, lyrics),
                requested_codec: selected_codec,
            },
        )
    })
    .await
    .map_err(|error| SubsonicError::generic(format, format!("playback task panicked: {error}")))?
    .map_err(|error| map_app_error(format, error.into()))?;

    let file_path = context.config.cache_dir.join(
        playback
            .relative_path
            .strip_prefix("cache/")
            .unwrap_or(&playback.relative_path),
    );
    ServeFile::new(file_path)
        .oneshot(request)
        .await
        .map(|response| response.map(axum::body::Body::new))
        .map_err(|error| SubsonicError::generic(format, error.to_string()))
}
