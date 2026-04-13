# Repository Guidelines

## Project Structure & Module Organization
`Cargo.toml` defines a three-crate workspace. `crates/apple-music-api` owns Apple Music request building and response parsing. `crates/apple-music-decryptor` holds the native session runtime, download pipeline, MP4 tagging, and C/C++ bridge code in `cpp/`. `crates/apple-music-downloader` is the daemon crate: `src/bin/main.rs` starts the server, `src/config/` parses CLI flags, `src/daemon/http/` serves REST routes, `src/daemon/subsonic/` serves Subsonic endpoints, and `src/runtime/` keeps session state. Root docs live in `README.md` and `API.md`; deployment assets live in `Dockerfile`, `.github/workflows/`, `k8s/`, and `scripts/`; end-to-end smoke coverage lives in `test/all.sh`.

## Build, Test, and Development Commands
`.cargo/config.toml` sets the default target to `x86_64-linux-android`.

- `cargo ndk -t x86_64 build --release --bin main` builds the Android daemon artifact.
- `HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"; cargo test --workspace --target "$HOST_TRIPLE"` runs unit tests on the local host target.
- `cargo fmt --all -- --check` validates formatting.
- `cargo clippy --workspace --all-targets --target "$HOST_TRIPLE" -- -D warnings` enforces a clean lint pass.
- `bash test/all.sh` runs the login, search, and playback smoke flow. Set `API_BEARER_TOKEN`, `APPLE_USERNAME`, `APPLE_PASSWORD`, and optional `APPLE_2FA` first.

## Coding Style & Naming Conventions
Use rustfmt defaults with 4-space indentation. Follow Rust naming rules: `snake_case` for modules and functions, `PascalCase` for types, and `SCREAMING_SNAKE_CASE` for constants. Keep logic in the crate that owns the domain instead of adding cross-crate utility sprawl. Write comments for FFI limits, runtime trade-offs, and business rules; keep straightforward control flow self-explanatory.

## Testing Guidelines
Place unit tests beside the implementation with `#[cfg(test)] mod tests`. Current examples in `apple-music-api/src/client/`, `apple-music-decryptor/src/ffi/runtime.rs`, and `apple-music-downloader/src/daemon/` show the expected pattern: deterministic inputs, direct assertions, and focused behavior names. Run host-target unit tests before opening a PR, then run `test/all.sh` when API, auth, playback, or cache behavior changes.

## Commit & Pull Request Guidelines
Recent history favors short imperative subjects with prefixes such as `feat:`, `fix:`, and `refactor:`. Keep commits in that style, for example `fix: preserve storefront in lyrics requests`. PRs should describe user impact, root cause, verification commands, and any API or deployment changes. Include request and response samples when HTTP or Subsonic output changes.

## Security & Configuration Tips
Keep Apple credentials, API tokens, and persisted app data out of version control. Treat `/data/data/com.apple.android.music`, cache directories, and extracted native libraries as local runtime state. When changing Docker or `k8s/` assets, update `README.md` and `API.md` in the same change set.
