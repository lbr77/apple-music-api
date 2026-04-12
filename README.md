# apple-music-api

Apple Music daemon for `x86_64-linux-android`.

This project boots an Android Apple Music runtime, exposes a small HTTP service, and supports local playback decryption plus a Subsonic-compatible interface. API details, request parameters, and response examples live in [`API.md`](./API.md).

> Notice
>
> Full lyrics require `MEDIA_USER_TOKEN`. In this daemon, provide it with `--media-user-token` or `WRAPPER_MEDIA_USER_TOKEN`.

## Deployment

### Runtime requirements

- Android Apple Music runtime libraries
- persistent app data at `/data/data/com.apple.android.music`
- `ffmpeg` and `ffprobe` at `/usr/local/bin`
- required startup flag: `--api-token`

The default runtime base directory is `/data/data/com.apple.android.music/files`. Persisting the parent directory is enough for login state and runtime data.

### Docker

The repository includes a multi-stage [Dockerfile](./Dockerfile) and CI publishes:

```text
ghcr.io/lbr77/apple-music-api:latest
```

Example:

```bash
docker run --rm -p 8080:8080 \
  -v ./persist/com.apple.android.music:/data/data/com.apple.android.music \
  ghcr.io/lbr77/apple-music-api:latest \
  --host 0.0.0.0 \
  --daemon-port 8080 \
  --api-token local-dev-token
```

## Local build

The workspace targets `x86_64-linux-android` by default.

```bash
rustup target add x86_64-linux-android
cargo install --locked cargo-ndk

export ANDROID_NDK_HOME=/path/to/android-ndk
cargo ndk -t x86_64 build --release --bin main
```

Output:

```text
target/x86_64-linux-android/release/main
```

Use [`API.md`](./API.md) for login flow, route list, request examples, Subsonic usage, and cache behavior.
