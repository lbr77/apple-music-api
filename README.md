# wrapper-rs

Rust rewrite of the original [wrapper](https://github.com/WorldObservationLog/wrapper) and [go-api](https://github.com/akinazuki/apple-music-downloader/blob/main/API.md) flow for `x86_64-linux-android`.

This repository currently ships two binaries:

- `main`: daemon runtime that serves the HTTP API
- `wrapper`: launcher that enters `./rootfs`, prepares runtime devices, and execs `/system/bin/main`

## Build

### Android target

```bash
ANDROID_NDK_HOME="/opt/homebrew/share/android-ndk" cargo ndk -t x86_64 build --release
```

The release binary is `target/x86_64-linux-android/release/wrapper`.

Canonical local build wrapper:

```bash
./scripts/build-android.sh
```

### Host build (for local debug)

```bash
cargo build --release
```

The daemon binary is `target/release/main`.

## Run

Run the daemon directly:

```bash
WRAPPER_SUBSONIC_USERNAME=admin \
WRAPPER_SUBSONIC_PASSWORD=admin123 \
./target/release/main --daemon-port 8080 --api-token local-dev-token
```

By default it binds to `127.0.0.1:8080`.

Quick health check:

```bash
curl -H "Authorization: Bearer local-dev-token" http://127.0.0.1:8080/health
```

`/health` also returns a `version` field, which is the first 8 characters of the git commit hash captured at build time.

All daemon endpoints require `Authorization: Bearer <api-token>`.

Subsonic-compatible `/rest/*.view` endpoints read credentials from environment variables instead of CLI flags:

- `WRAPPER_SUBSONIC_USERNAME` defaults to `admin`
- `WRAPPER_SUBSONIC_PASSWORD` defaults to `admin123`

The daemon expects `ffmpeg` and `ffprobe` at `/usr/local/bin`.
Playback assembly uses `ffmpeg` for audio remux and writes final MP4 metadata directly in Rust.

## Deployment

For runtime persistence, mount and persist only this directory:

```text
/data/data/com.apple.android.music
```

The runtime default `--base-dir` points to `/data/data/com.apple.android.music/files`, so mounting the parent directory above is enough to preserve runtime data.
When that directory survives a restart, the Rust daemon attempts to restore the previous login state during startup before serving requests.

Container examples:

```bash
docker run --rm -p 8080:8080 \
	-v ./persist/com.apple.android.music:/data/data/com.apple.android.music \
	ghcr.io/<owner>/<repo>:latest \
	--host 0.0.0.0 --daemon-port 8080
```

```yaml
services:
	wrapper:
		image: ghcr.io/<owner>/<repo>:latest
		command: ["--host", "0.0.0.0", "--daemon-port", "8080"]
		ports:
			- "8080:8080"
		volumes:
			- ./persist/com.apple.android.music:/data/data/com.apple.android.music
		restart: unless-stopped
```

## HTTP API


Error responses also return JSON. The request outcome is always carried by `status`, and login/session state remains in `state` when it is relevant.

```json
{"status":"error","state":"logged_out","message":"no active session"}
```

`status` values for error responses are always `error`; the daemon does not use `status: "logged_out"`.

Full request/response examples are documented in [API.md](API.md).

## Login and 2FA Flow

### 1) Start login

```bash
curl -X POST http://127.0.0.1:8080/login \
	-H 'Authorization: Bearer local-dev-token' \
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
	-H 'Authorization: Bearer local-dev-token' \
	-H 'content-type: application/json' \
	-d '{"code":"123456"}'
```

Success result:

```json
{"status":"ok","state":"logged_in"}
```

### 3) Check status

```bash
curl -H "Authorization: Bearer local-dev-token" http://127.0.0.1:8080/status
```

### 4) Logout

```bash
curl -X POST http://127.0.0.1:8080/logout \
	-H 'Authorization: Bearer local-dev-token'
```

## Cache Layout

- Lyrics: `./cache/lyrics/<songId>.lrc`
- Audio: `./cache/albums/<albumId>/<songId>.m4a`

`GET /playback/{id}?redirect=true` returns HTTP 302 to the cached `.m4a` file under `/cache/...`.
When lyrics are available from Apple Music, the cached `.m4a` also embeds them as MP4 metadata.

The Docker image already bundles `ffmpeg` and `ffprobe`, so `/health` exposes both tool reports at runtime.

## Legacy TCP Notes

The previous one-request-per-connection control protocol and raw decrypt TCP framing are legacy behavior from earlier runtime wiring.

For current integration, use the HTTP API above as the primary control surface.
