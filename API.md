# Apple Music Downloader Daemon API

Base URL: `http://localhost:<port>`

All endpoints require `Authorization: Bearer <api-token>`.

---

## GET /health

Report daemon state, build version, and external media tool availability from the fixed `/usr/local/bin` runtime paths.

`ffmpeg` and `ffprobe` are required for a healthy status. Playback uses `ffmpeg` for audio remux and writes final MP4 metadata directly in Rust.

**Example**

```bash
curl -H "Authorization: Bearer <api-token>" "http://localhost:8080/health"
```

```json
{
  "status": "ok",
  "state": "logged_in",
  "version": "1a2b3c4d",
  "ffmpeg": { "path": "/usr/local/bin/ffmpeg", "available": true, "version": "ffmpeg version 7.0.2-static" },
  "ffprobe": { "path": "/usr/local/bin/ffprobe", "available": true, "version": "ffprobe version 7.0.2-static" }
}
```

---

## GET /search

Search for songs, albums, or artists.

**Query Parameters**

| Parameter | Required | Default | Description |
|-----------|----------|---------|-------------|
| `query`   | Yes      | —       | Search keyword |
| `type`    | No       | `song`  | `song`, `album`, or `artist` |
| `limit`   | No       | `10`    | Number of results |
| `offset`  | No       | `0`     | Pagination offset |

**Example**

```bash
curl -H "Authorization: Bearer <api-token>" "http://localhost:8080/search?query=IOSYS&type=album&limit=2"
```

```json
{
  "results": {
    "albums": {
      "href": "/v1/catalog/jp/search?l=ja&limit=2&offset=0&term=IOSYS&types=albums",
      "next": "/v1/catalog/jp/search?l=ja&offset=2&term=IOSYS&types=albums",
      "data": [
        {
          "id": "1480785394",
          "type": "albums",
          "href": "/v1/catalog/jp/albums/1480785394?l=ja",
          "attributes": {
            "artistName": "IOSYS",
            "name": "miko BEST Toho of IOSYS",
            "trackCount": 24,
            "releaseDate": "2013-12-11",
            "audioTraits": ["lossless", "lossy-stereo"]
          }
        }
      ]
    }
  }
}
```

---

## GET /artist/:id

Fetch artist metadata plus the default Apple Music artist-page views.

By default this endpoint asks Apple Music for:
`top-songs`, `latest-release`, `full-albums`, `singles`,
`featured-playlists`, `playlists`, `similar-artists`, and
`top-music-videos`.

**Path Parameters**

| Parameter | Description |
|-----------|-------------|
| `id`      | Apple Music artist ID |

**Query Parameters**

| Parameter    | Default        | Description |
|--------------|----------------|-------------|
| `storefront` | Config default | Storefront region |
| `language`   | Config default | Raw Apple Music language query fragment. Plain values become `l=<value>`; structured values like `l[lyrics]=zh-hans-cn&l[script]=zh-Hans` are passed through unchanged. |
| `views`      | Built-in list  | Comma-separated artist views to request from Apple Music |
| `limit`      | Apple default  | Per-view item limit applied by Apple Music |

**Example**

```bash
curl -H "Authorization: Bearer <api-token>" "http://localhost:8080/artist/287018328"
```

```json
{
  "data": [
    {
      "id": "287018328",
      "type": "artists",
      "attributes": {
        "name": "IOSYS",
        "url": "https://music.apple.com/jp/artist/iosys/287018328"
      },
      "relationships": {
        "genres": { "data": [{ "id": "34", "type": "genres", "attributes": { "name": "J-Pop" } }] },
        "station": { "data": [{ "id": "ra.123", "type": "stations" }] }
      },
      "views": {
        "top-songs": { "data": [{ "id": "1480785395", "type": "songs" }] },
        "latest-release": { "data": [{ "id": "1480785394", "type": "albums" }] },
        "full-albums": { "data": [{ "id": "1480785394", "type": "albums" }] },
        "similar-artists": { "data": [{ "id": "123456789", "type": "artists" }] }
      }
    }
  ]
}
```

---

## GET /artist/:id/view/:name

Fetch a single artist view directly, which is useful for independent section pagination on an artist page.

Examples of `name` include `top-songs`, `full-albums`, `singles`,
`featured-playlists`, `playlists`, `similar-artists`, and
`top-music-videos`.

**Path Parameters**

| Parameter | Description |
|-----------|-------------|
| `id`      | Apple Music artist ID |
| `name`    | Apple Music artist view name |

**Query Parameters**

| Parameter    | Default        | Description |
|--------------|----------------|-------------|
| `storefront` | Config default | Storefront region |
| `limit`      | Apple default  | Number of items |
| `offset`     | `0`            | Pagination offset |

**Example**

```bash
curl -H "Authorization: Bearer <api-token>" "http://localhost:8080/artist/287018328/view/full-albums?limit=25&offset=0"
```

```json
{
  "href": "/v1/catalog/jp/artists/287018328/view/full-albums?limit=25&offset=0&l=ja",
  "next": "/v1/catalog/jp/artists/287018328/view/full-albums?limit=25&offset=25&l=ja",
  "data": [
    {
      "id": "1480785394",
      "type": "albums",
      "attributes": {
        "artistName": "IOSYS",
        "name": "miko BEST Toho of IOSYS"
      }
    }
  ]
}
```

---

## GET /album/:id

Fetch album metadata and full track list.

**Path Parameters**

| Parameter | Description |
|-----------|-------------|
| `id`      | Apple Music album ID |

**Query Parameters**

| Parameter    | Default        | Description |
|--------------|----------------|-------------|
| `storefront` | Config default | Storefront region (e.g. `jp`, `us`) |

**Example**

```bash
curl -H "Authorization: Bearer <api-token>" "http://localhost:8080/album/1480785394"
```

```json
{
  "href": "",
  "next": "",
  "data": [
    {
      "id": "1480785394",
      "type": "albums",
      "href": "/v1/catalog/jp/albums/1480785394?l=ja",
      "attributes": {
        "artistName": "IOSYS",
        "name": "miko BEST Toho of IOSYS",
        "trackCount": 24,
        "releaseDate": "2013-12-11",
        "recordLabel": "東方同人音楽流通",
        "upc": "4580547320671",
        "copyright": "℗ 2013 IOSYS",
        "genreNames": ["J-Pop", "ミュージック"],
        "audioTraits": ["lossless", "lossy-stereo"],
        "isSingle": false,
        "isComplete": true,
        "isCompilation": true,
        "artwork": {
          "width": 2500,
          "height": 2500,
          "url": "https://is1-ssl.mzstatic.com/image/thumb/.../{w}x{h}bb.jpg"
        },
        "playParams": { "id": "1480785394", "kind": "album" }
      },
      "relationships": {
        "artists": {
          "href": "/v1/catalog/jp/albums/1480785394/artists?l=ja",
          "data": [
            {
              "id": "287018328",
              "type": "artists",
              "attributes": {
                "name": "IOSYS",
                "artwork": { "url": "https://is1-ssl.mzstatic.com/image/thumb/.../{w}x{h}ac.jpg" }
              }
            }
          ]
        },
        "tracks": {
          "href": "/v1/catalog/jp/albums/1480785394/tracks?l=ja",
          "data": [
            {
              "id": "1480785395",
              "type": "songs",
              "href": "/v1/catalog/jp/songs/1480785395?l=ja",
              "attributes": {
                "name": "魔理沙は大変なものを盗んでいきました",
                "artistName": "IOSYS",
                "albumName": "miko BEST Toho of IOSYS",
                "trackNumber": 1,
                "discNumber": 1,
                "durationInMillis": 240618,
                "releaseDate": "2013-12-11",
                "isrc": "JPI961900138",
                "composerName": "ZUN & IOSYS",
                "audioTraits": ["lossless", "lossy-stereo"],
                "hasLyrics": true,
                "hasTimeSyncedLyrics": true,
                "previews": [{ "url": "https://audio-ssl.itunes.apple.com/..." }],
                "extendedAssetUrls": { "enhancedHls": "https://aod.itunes.apple.com/..." }
              }
            }
          ]
        }
      }
    }
  ]
}
```

---

## GET /song/:id

Fetch song metadata.

**Path Parameters**

| Parameter | Description |
|-----------|-------------|
| `id`      | Apple Music song ID |

**Query Parameters**

| Parameter    | Default        | Description |
|--------------|----------------|-------------|
| `storefront` | Config default | Storefront region |

**Example**

```bash
curl -H "Authorization: Bearer <api-token>" "http://localhost:8080/song/1480785411"
```

```json
{
  "href": "",
  "next": "",
  "data": [
    {
      "id": "1480785411",
      "type": "songs",
      "href": "/v1/catalog/jp/songs/1480785411?l=ja",
      "attributes": {
        "name": "記憶の系譜 ~ until the End of History",
        "artistName": "IOSYS",
        "albumName": "miko BEST Toho of IOSYS",
        "trackNumber": 17,
        "discNumber": 1,
        "durationInMillis": 374605,
        "releaseDate": "2013-12-11",
        "isrc": "JPI961900154",
        "composerName": "ZUN & IOSYS",
        "audioTraits": ["lossless", "lossy-stereo"],
        "hasLyrics": false,
        "hasTimeSyncedLyrics": false,
        "previews": [{ "url": "https://audio-ssl.itunes.apple.com/..." }],
        "extendedAssetUrls": { "enhancedHls": "https://aod.itunes.apple.com/..." },
        "playParams": { "id": "1480785411", "kind": "song" }
      },
      "relationships": {
        "artists": {
          "data": [{ "id": "287018328", "type": "artists", "attributes": { "name": "IOSYS" } }]
        }
      }
    }
  ]
}
```

---

## GET /playback/:id

Download and cache the audio file for a song. Returns playback info once the file is ready.

The file is cached at `./cache/albums/<albumId>/<songId>.m4a`.
When lyrics are available for the song, the cached `.m4a` also embeds them as MP4 metadata.

**Path Parameters**

| Parameter | Description |
|-----------|-------------|
| `id`      | Apple Music song ID |

**Query Parameters**

| Parameter    | Default        | Description |
|--------------|----------------|-------------|
| `storefront` | Config default | Storefront region |
| `redirect`   | `false`        | If `true`, 302 redirects to the cached `.m4a` file |

**Example**

```bash
curl -H "Authorization: Bearer <api-token>" "http://localhost:8080/playback/1480785411"
```

```json
{
  "playbackUrl": "cache/albums/1480785394/1480785411.m4a",
  "size": 77092711,
  "title": "記憶の系譜 ~ until the End of History",
  "artist": "IOSYS",
  "artistId": "287018328",
  "album": "miko BEST Toho of IOSYS",
  "albumId": "1480785394",
  "codec": "ALAC"
}
```

With redirect:

```bash
curl -L -H "Authorization: Bearer <api-token>" "http://localhost:8080/playback/1480785411?redirect=true"
# 302 → /cache/albums/1480785394/1480785411.m4a
```

---

## GET /lyrics/:id

Fetch lyrics for a song in LRC format. Results are cached at `./cache/lyrics/<songId>.lrc`.

**Path Parameters**

| Parameter | Description |
|-----------|-------------|
| `id`      | Apple Music song ID |

**Query Parameters**

| Parameter    | Default        | Description |
|--------------|----------------|-------------|
| `storefront` | Config default | Storefront region |

**Example**

```bash
curl -H "Authorization: Bearer <api-token>" "http://localhost:8080/lyrics/1480785411"
```

Chinese syllable lyrics can be requested explicitly:

```bash
curl -G -H "Authorization: Bearer <api-token>" \
  --data-urlencode "storefront=cn" \
  --data-urlencode "language=l[lyrics]=zh-hans-cn&l[script]=zh-Hans" \
  "http://localhost:8080/lyrics/1648869428"
```

```json
{
  "lyrics": "[00:01.00]Line one\n[00:04.00]Line two\n..."
}
```

---

## Static Files

Cached files are served at `/cache`:

```
GET /cache/albums/<albumId>/<songId>.m4a
GET /cache/lyrics/<songId>.lrc
```
