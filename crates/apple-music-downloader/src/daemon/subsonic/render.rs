use axum::Json;
use axum::http::header::{CONTENT_TYPE, HeaderValue};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use super::data::{SubsonicAlbum, SubsonicArtist, SubsonicSong, music_folder_id};
use super::{ResponseFormat, SUBSONIC_API_VERSION, SUBSONIC_SERVER_TYPE};

pub(super) fn subsonic_ok_response(
    format: ResponseFormat,
    json_body: Option<Value>,
    xml_fragment: &str,
) -> Response {
    match format {
        ResponseFormat::Json => subsonic_ok_json(json_body.unwrap_or_else(|| json!({}))),
        ResponseFormat::Xml => subsonic_ok_xml(xml_fragment),
    }
}

pub(super) fn subsonic_ok_json(payload: Value) -> Response {
    let mut response = serde_json::Map::new();
    response.insert("status".into(), json!("ok"));
    response.insert("version".into(), json!(SUBSONIC_API_VERSION));
    response.insert("type".into(), json!(SUBSONIC_SERVER_TYPE));
    response.insert("serverVersion".into(), json!(crate::BUILD_VERSION));
    if let Some(object) = payload.as_object() {
        response.extend(object.clone());
    }
    Json(json!({
        "subsonic-response": Value::Object(response)
    }))
    .into_response()
}

pub(super) fn subsonic_ok_xml(fragment: &str) -> Response {
    (
        [(CONTENT_TYPE, HeaderValue::from_static("application/xml; charset=utf-8"))],
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><subsonic-response xmlns="http://subsonic.org/restapi" status="ok" version="{SUBSONIC_API_VERSION}" type="{SUBSONIC_SERVER_TYPE}" serverVersion="{}">{fragment}</subsonic-response>"#,
            crate::BUILD_VERSION,
        ),
    )
        .into_response()
}

pub(super) fn subsonic_error_response(
    format: ResponseFormat,
    code: i32,
    message: &str,
) -> Response {
    match format {
        ResponseFormat::Json => Json(json!({
            "subsonic-response": {
                "status": "failed",
                "version": SUBSONIC_API_VERSION,
                "type": SUBSONIC_SERVER_TYPE,
                "serverVersion": crate::BUILD_VERSION,
                "error": {
                    "code": code,
                    "message": message,
                }
            }
        }))
        .into_response(),
        ResponseFormat::Xml => (
            [(CONTENT_TYPE, HeaderValue::from_static("application/xml; charset=utf-8"))],
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><subsonic-response xmlns="http://subsonic.org/restapi" status="failed" version="{SUBSONIC_API_VERSION}" type="{SUBSONIC_SERVER_TYPE}" serverVersion="{}"><error code="{code}" message="{}"/></subsonic-response>"#,
                crate::BUILD_VERSION,
                escape_xml_attr(message),
            ),
        )
            .into_response(),
    }
}

pub(super) fn artist_json(artist: &SubsonicArtist) -> Value {
    let mut value = serde_json::Map::new();
    value.insert("id".into(), json!(artist.id));
    value.insert("name".into(), json!(artist.name));
    if let Some(album_count) = artist.album_count {
        value.insert("albumCount".into(), json!(album_count));
    }
    if let Some(cover_art) = artist.cover_art.as_deref() {
        value.insert("coverArt".into(), json!(cover_art));
    }
    if let Some(image_url) = artist.artist_image_url.as_deref() {
        value.insert("artistImageUrl".into(), json!(image_url));
    }
    Value::Object(value)
}

pub(super) fn album_json(album: &SubsonicAlbum) -> Value {
    let mut value = serde_json::Map::new();
    value.insert("id".into(), json!(album.id));
    value.insert("name".into(), json!(album.name));
    value.insert("artist".into(), json!(album.artist));
    value.insert("musicFolderId".into(), json!(music_folder_id()));
    if let Some(artist_id) = album.artist_id.as_deref() {
        value.insert("artistId".into(), json!(artist_id));
    }
    if let Some(cover_art) = album.cover_art.as_deref() {
        value.insert("coverArt".into(), json!(cover_art));
    }
    if let Some(song_count) = album.song_count {
        value.insert("songCount".into(), json!(song_count));
    }
    if let Some(duration) = album.duration {
        value.insert("duration".into(), json!(duration));
    }
    if let Some(year) = album.year {
        value.insert("year".into(), json!(year));
    }
    if let Some(created) = album.created.as_deref() {
        value.insert("created".into(), json!(created));
    }
    if let Some(genre) = album.genre.as_deref() {
        value.insert("genre".into(), json!(genre));
    }
    Value::Object(value)
}

pub(super) fn song_json(song: &SubsonicSong) -> Value {
    let mut value = serde_json::Map::new();
    value.insert("id".into(), json!(song.id));
    value.insert("isDir".into(), json!(false));
    value.insert("title".into(), json!(song.title));
    value.insert("artist".into(), json!(song.artist));
    value.insert("suffix".into(), json!(song.suffix));
    value.insert("contentType".into(), json!(song.content_type));
    value.insert("musicFolderId".into(), json!(music_folder_id()));
    if let Some(parent) = song.parent.as_deref() {
        value.insert("parent".into(), json!(parent));
    }
    if let Some(album) = song.album.as_deref() {
        value.insert("album".into(), json!(album));
    }
    if let Some(artist_id) = song.artist_id.as_deref() {
        value.insert("artistId".into(), json!(artist_id));
    }
    if let Some(cover_art) = song.cover_art.as_deref() {
        value.insert("coverArt".into(), json!(cover_art));
    }
    if let Some(duration) = song.duration {
        value.insert("duration".into(), json!(duration));
    }
    if let Some(track) = song.track {
        value.insert("track".into(), json!(track));
    }
    if let Some(disc_number) = song.disc_number {
        value.insert("discNumber".into(), json!(disc_number));
    }
    if let Some(year) = song.year {
        value.insert("year".into(), json!(year));
    }
    if let Some(created) = song.created.as_deref() {
        value.insert("created".into(), json!(created));
    }
    if let Some(genre) = song.genre.as_deref() {
        value.insert("genre".into(), json!(genre));
    }
    if let Some(album_id) = song.album_id.as_deref() {
        value.insert("albumId".into(), json!(album_id));
    }
    Value::Object(value)
}

pub(super) fn artist_xml(artist: &SubsonicArtist) -> String {
    format!("<artist{}/>", artist_attrs(artist))
}

fn artist_attrs(artist: &SubsonicArtist) -> String {
    let mut attrs = format!(
        r#" id="{}" name="{}""#,
        escape_xml_attr(&artist.id),
        escape_xml_attr(&artist.name),
    );
    if let Some(album_count) = artist.album_count {
        attrs.push_str(&format!(r#" albumCount="{album_count}""#));
    }
    if let Some(cover_art) = artist.cover_art.as_deref() {
        attrs.push_str(&format!(r#" coverArt="{}""#, escape_xml_attr(cover_art)));
    }
    if let Some(image_url) = artist.artist_image_url.as_deref() {
        attrs.push_str(&format!(
            r#" artistImageUrl="{}""#,
            escape_xml_attr(image_url)
        ));
    }
    attrs
}

pub(super) fn album_xml(album: &SubsonicAlbum) -> String {
    format!("<album{}/>", album_attrs(album))
}

fn album_attrs(album: &SubsonicAlbum) -> String {
    let mut attrs = format!(
        r#" id="{}" name="{}" artist="{}" musicFolderId="{}""#,
        escape_xml_attr(&album.id),
        escape_xml_attr(&album.name),
        escape_xml_attr(&album.artist),
        music_folder_id(),
    );
    if let Some(artist_id) = album.artist_id.as_deref() {
        attrs.push_str(&format!(r#" artistId="{}""#, escape_xml_attr(artist_id)));
    }
    if let Some(cover_art) = album.cover_art.as_deref() {
        attrs.push_str(&format!(r#" coverArt="{}""#, escape_xml_attr(cover_art)));
    }
    if let Some(song_count) = album.song_count {
        attrs.push_str(&format!(r#" songCount="{song_count}""#));
    }
    if let Some(duration) = album.duration {
        attrs.push_str(&format!(r#" duration="{duration}""#));
    }
    if let Some(year) = album.year {
        attrs.push_str(&format!(r#" year="{year}""#));
    }
    if let Some(created) = album.created.as_deref() {
        attrs.push_str(&format!(r#" created="{}""#, escape_xml_attr(created)));
    }
    if let Some(genre) = album.genre.as_deref() {
        attrs.push_str(&format!(r#" genre="{}""#, escape_xml_attr(genre)));
    }
    attrs
}

pub(super) fn song_xml(song: &SubsonicSong) -> String {
    format!("<song{}/>", song_attrs(song))
}

fn song_attrs(song: &SubsonicSong) -> String {
    let mut attrs = format!(
        r#" id="{}" isDir="false" title="{}" artist="{}" suffix="{}" contentType="{}" musicFolderId="{}""#,
        escape_xml_attr(&song.id),
        escape_xml_attr(&song.title),
        escape_xml_attr(&song.artist),
        song.suffix,
        song.content_type,
        music_folder_id(),
    );
    if let Some(parent) = song.parent.as_deref() {
        attrs.push_str(&format!(r#" parent="{}""#, escape_xml_attr(parent)));
    }
    if let Some(album) = song.album.as_deref() {
        attrs.push_str(&format!(r#" album="{}""#, escape_xml_attr(album)));
    }
    if let Some(artist_id) = song.artist_id.as_deref() {
        attrs.push_str(&format!(r#" artistId="{}""#, escape_xml_attr(artist_id)));
    }
    if let Some(cover_art) = song.cover_art.as_deref() {
        attrs.push_str(&format!(r#" coverArt="{}""#, escape_xml_attr(cover_art)));
    }
    if let Some(duration) = song.duration {
        attrs.push_str(&format!(r#" duration="{duration}""#));
    }
    if let Some(track) = song.track {
        attrs.push_str(&format!(r#" track="{track}""#));
    }
    if let Some(disc_number) = song.disc_number {
        attrs.push_str(&format!(r#" discNumber="{disc_number}""#));
    }
    if let Some(year) = song.year {
        attrs.push_str(&format!(r#" year="{year}""#));
    }
    if let Some(created) = song.created.as_deref() {
        attrs.push_str(&format!(r#" created="{}""#, escape_xml_attr(created)));
    }
    if let Some(genre) = song.genre.as_deref() {
        attrs.push_str(&format!(r#" genre="{}""#, escape_xml_attr(genre)));
    }
    if let Some(album_id) = song.album_id.as_deref() {
        attrs.push_str(&format!(r#" albumId="{}""#, escape_xml_attr(album_id)));
    }
    attrs
}

pub(super) fn escape_xml_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(super) fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
