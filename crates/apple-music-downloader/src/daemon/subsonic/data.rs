use std::collections::HashSet;

use serde_json::Value;

use super::SUBSONIC_MUSIC_FOLDER_ID;

#[derive(Clone)]
pub(super) struct SubsonicArtist {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) cover_art: Option<String>,
    pub(super) artist_image_url: Option<String>,
    pub(super) album_count: Option<usize>,
}

#[derive(Clone)]
pub(super) struct SubsonicAlbum {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) artist: String,
    pub(super) artist_id: Option<String>,
    pub(super) cover_art: Option<String>,
    pub(super) song_count: Option<usize>,
    pub(super) duration: Option<u64>,
    pub(super) year: Option<i32>,
    pub(super) created: Option<String>,
    pub(super) genre: Option<String>,
}

#[derive(Clone)]
pub(super) struct SubsonicSong {
    pub(super) id: String,
    pub(super) parent: Option<String>,
    pub(super) title: String,
    pub(super) album: Option<String>,
    pub(super) artist: String,
    pub(super) artist_id: Option<String>,
    pub(super) cover_art: Option<String>,
    pub(super) duration: Option<u64>,
    pub(super) track: Option<u32>,
    pub(super) disc_number: Option<u32>,
    pub(super) year: Option<i32>,
    pub(super) created: Option<String>,
    pub(super) genre: Option<String>,
    pub(super) suffix: &'static str,
    pub(super) content_type: &'static str,
    pub(super) album_id: Option<String>,
}

pub(super) struct SubsonicLyrics {
    pub(super) artist: String,
    pub(super) title: String,
    pub(super) value: String,
}

pub(super) fn search_results<'a>(value: &'a Value, pointer: &str) -> &'a [Value] {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

pub(super) fn string_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

fn u64_at(value: &Value, pointer: &str) -> Option<u64> {
    value.pointer(pointer).and_then(Value::as_u64)
}

fn u32_at(value: &Value, pointer: &str) -> Option<u32> {
    u64_at(value, pointer).and_then(|value| u32::try_from(value).ok())
}

fn created_at(value: &Value, pointer: &str) -> Option<String> {
    string_at(value, pointer).map(|date| format!("{date}T00:00:00"))
}

fn release_year(value: &Value, pointer: &str) -> Option<i32> {
    string_at(value, pointer)
        .and_then(|date| date.get(..4))
        .and_then(|year| year.parse::<i32>().ok())
}

fn first_genre(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .and_then(|genres| genres.first())
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub(super) fn artwork_template<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    string_at(value, pointer).filter(|value| !value.is_empty())
}

pub(super) fn render_artwork_url(template: &str, size: Option<u32>) -> String {
    let width = size.unwrap_or(1200).max(1);
    let height = size.unwrap_or(width).max(1);
    template
        .replace("{w}", &width.to_string())
        .replace("{h}", &height.to_string())
}

pub(super) fn search_artist_to_subsonic(value: &Value) -> SubsonicArtist {
    let id = string_at(value, "/id").unwrap_or_default().to_owned();
    let name = string_at(value, "/attributes/name")
        .unwrap_or_default()
        .to_owned();
    let has_artwork = artwork_template(value, "/attributes/artwork/url").is_some();
    SubsonicArtist {
        id: id.clone(),
        name,
        cover_art: has_artwork.then_some(id.clone()),
        artist_image_url: artwork_template(value, "/attributes/artwork/url")
            .map(|template| render_artwork_url(template, None)),
        album_count: None,
    }
}

pub(super) fn search_album_to_subsonic(value: &Value) -> SubsonicAlbum {
    let id = string_at(value, "/id").unwrap_or_default().to_owned();
    SubsonicAlbum {
        id: id.clone(),
        name: string_at(value, "/attributes/name")
            .unwrap_or_default()
            .to_owned(),
        artist: string_at(value, "/attributes/artistName")
            .unwrap_or_default()
            .to_owned(),
        artist_id: None,
        cover_art: artwork_template(value, "/attributes/artwork/url").map(|_| id),
        song_count: u64_at(value, "/attributes/trackCount")
            .and_then(|count| usize::try_from(count).ok()),
        duration: None,
        year: release_year(value, "/attributes/releaseDate"),
        created: created_at(value, "/attributes/releaseDate"),
        genre: first_genre(value, "/attributes/genreNames"),
    }
}

pub(super) fn search_song_to_subsonic(value: &Value) -> SubsonicSong {
    let album_id = string_at(value, "/relationships/albums/data/0/id").map(str::to_owned);
    let cover_art = album_id.clone().or_else(|| {
        artwork_template(value, "/attributes/artwork/url")
            .is_some()
            .then(|| string_at(value, "/id").unwrap_or_default().to_owned())
    });
    SubsonicSong {
        id: string_at(value, "/id").unwrap_or_default().to_owned(),
        parent: album_id.clone(),
        title: string_at(value, "/attributes/name")
            .unwrap_or_default()
            .to_owned(),
        album: string_at(value, "/attributes/albumName").map(str::to_owned),
        artist: string_at(value, "/attributes/artistName")
            .unwrap_or_default()
            .to_owned(),
        artist_id: string_at(value, "/relationships/artists/data/0/id").map(str::to_owned),
        cover_art,
        duration: u64_at(value, "/attributes/durationInMillis").map(|millis| millis / 1000),
        track: u32_at(value, "/attributes/trackNumber"),
        disc_number: u32_at(value, "/attributes/discNumber"),
        year: release_year(value, "/attributes/releaseDate"),
        created: created_at(value, "/attributes/releaseDate"),
        genre: first_genre(value, "/attributes/genreNames"),
        suffix: "m4a",
        content_type: "audio/mp4",
        album_id,
    }
}

pub(super) fn album_detail_to_subsonic(value: &Value) -> Option<SubsonicAlbum> {
    let id = string_at(value, "/data/0/id")?.to_owned();
    Some(SubsonicAlbum {
        id: id.clone(),
        name: string_at(value, "/data/0/attributes/name")?.to_owned(),
        artist: string_at(value, "/data/0/attributes/artistName")
            .unwrap_or_default()
            .to_owned(),
        artist_id: string_at(value, "/data/0/relationships/artists/data/0/id").map(str::to_owned),
        cover_art: artwork_template(value, "/data/0/attributes/artwork/url").map(|_| id),
        song_count: Some(search_results(value, "/data/0/relationships/tracks/data").len()),
        duration: None,
        year: release_year(value, "/data/0/attributes/releaseDate"),
        created: created_at(value, "/data/0/attributes/releaseDate"),
        genre: first_genre(value, "/data/0/attributes/genreNames"),
    })
}

pub(super) fn album_track_to_subsonic(value: &Value) -> SubsonicSong {
    let album_id = string_at(value, "/attributes/playParams/catalogId")
        .or_else(|| string_at(value, "/relationships/albums/data/0/id"))
        .map(str::to_owned);
    SubsonicSong {
        id: string_at(value, "/id").unwrap_or_default().to_owned(),
        parent: album_id.clone(),
        title: string_at(value, "/attributes/name")
            .unwrap_or_default()
            .to_owned(),
        album: string_at(value, "/attributes/albumName").map(str::to_owned),
        artist: string_at(value, "/attributes/artistName")
            .unwrap_or_default()
            .to_owned(),
        artist_id: string_at(value, "/relationships/artists/data/0/id").map(str::to_owned),
        cover_art: album_id.clone(),
        duration: u64_at(value, "/attributes/durationInMillis").map(|millis| millis / 1000),
        track: u32_at(value, "/attributes/trackNumber"),
        disc_number: u32_at(value, "/attributes/discNumber"),
        year: release_year(value, "/attributes/releaseDate"),
        created: created_at(value, "/attributes/releaseDate"),
        genre: first_genre(value, "/attributes/genreNames"),
        suffix: "m4a",
        content_type: "audio/mp4",
        album_id,
    }
}

pub(super) fn unique_artist_albums(value: &Value) -> (String, Option<String>, Vec<SubsonicAlbum>) {
    let artist_name = string_at(value, "/data/0/attributes/name")
        .unwrap_or_default()
        .to_owned();
    let artist_image_url = artwork_template(value, "/data/0/attributes/artwork/url")
        .map(|template| render_artwork_url(template, None));

    let mut seen_albums = HashSet::new();
    let mut albums = Vec::new();
    for path in [
        "/data/0/views/full-albums/data",
        "/data/0/views/singles/data",
        "/data/0/views/latest-release/data",
    ] {
        for item in search_results(value, path) {
            let album = search_album_to_subsonic(item);
            if seen_albums.insert(album.id.clone()) {
                albums.push(album);
            }
        }
    }

    (artist_name, artist_image_url, albums)
}

pub(super) fn music_folder_id() -> i32 {
    SUBSONIC_MUSIC_FOLDER_ID
}
