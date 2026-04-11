# wrapper-rs

Rust rewrite of the original [wrapper](https://github.com/WorldObservationLog/wrapper) and [go-api](https://github.com/akinazuki/apple-music-downloader/blob/main/API.md) flow for `x86_64-linux-android`.

This repository currently ships two binaries:

- `main`: daemon runtime that serves the HTTP API
- `wrapper`: launcher that enters `./rootfs`, prepares runtime devices, and execs `/system/bin/main`

## Build

### Android target

```bash
cargo ndk -t x86_64 build --release
```

The release binary is `target/x86_64-linux-android/release/wrapper`.

### Host build (for local debug)

```bash
cargo build --release
```

The daemon binary is `target/release/main`.

## Run

Run the daemon directly:

```bash
./target/release/main --daemon-port 8080
```

By default it binds to `127.0.0.1:8080`.

Quick health check:

```bash
curl http://127.0.0.1:8080/health
```

## Runtime Options

`main` accepts these key CLI flags:

| Flag | Default | Description |
|---|---|---|
| `--host`, `-H` | `127.0.0.1` | Bind address |
| `--daemon-port` | `8080` | HTTP daemon port |
| `--proxy`, `-P` | _(none)_ | Upstream proxy used by Apple API client |
| `--base-dir`, `-B` | `/data/data/com.apple.android.music/files` | Base data dir for native runtime |
| `--lib-dir` | auto-detect | Rootfs library directory override |
| `--cache-dir` | `cache` | Cache output directory |
| `--storefront` | `us` | Default storefront |
| `--language` | `""` | Optional Apple API language (`l` query) |
| `--device-info`, `-I` | preset value | Device profile passed to native layer |
| `--decrypt-workers` | `clamp(num_cpus, 2..8)` | Decrypt worker count |
| `--decrypt-inflight` | `max(2, workers * 2)` | Max queued decrypt jobs |

If `--lib-dir` is not specified, the runtime tries:

1. `/system/lib64`
2. `rootfs/system/lib64`
3. `./rootfs/system/lib64`

## HTTP API

The daemon exposes a JSON HTTP API.

Core endpoints:

- `GET /health`
- `GET /status`
- `POST /login`
- `POST /login/2fa`
- `POST /login/reset`
- `POST /logout`
- `GET /search`
- `GET /album/{id}`
- `GET /song/{id}`
- `GET /lyrics/{id}`
- `GET /playback/{id}`
- `GET /cache/...` (static cached files)

Full request/response examples are documented in [API.md](API.md).

## Login and 2FA Flow

### 1) Start login

```bash
curl -X POST http://127.0.0.1:8080/login \
	-H 'content-type: application/json' \
	-d '{"username":"apple@example.com","password":"secret"}'
```

Possible result:

```json
{"status":"need_2fa","state":"awaiting_2fa","message":"verification code required"}
```

### 2) Submit 2FA code

```bash
curl -X POST http://127.0.0.1:8080/login/2fa \
	-H 'content-type: application/json' \
	-d '{"code":"123456"}'
```

Success result:

```json
{"status":"ok","state":"logged_in"}
```

### 3) Check status

```bash
curl http://127.0.0.1:8080/status
```

### 4) Logout

```bash
curl -X POST http://127.0.0.1:8080/logout
```

## Cache Layout

- Lyrics: `./cache/lyrics/<songId>.lrc`
- Audio: `./cache/albums/<albumId>/<songId>.m4a`

`GET /playback/{id}?redirect=true` returns HTTP 302 to the cached `.m4a` file under `/cache/...`.

## Legacy TCP Notes

The previous one-request-per-connection control protocol and raw decrypt TCP framing are legacy behavior from earlier runtime wiring.

For current integration, use the HTTP API above as the primary control surface.
