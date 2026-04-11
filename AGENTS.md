# Repository Guidelines

## Project Structure & Module Organization
`src/bin/main.rs` starts the HTTP daemon, and `src/bin/wrapper.rs` launches the Android runtime inside `rootfs`. Shared logic lives in `src/`: `config/` parses CLI and device settings, `daemon/` serves API and download flows, `ffi/` binds the native Android libraries, `runtime/` owns session state, and `logging/` centralizes log output. Native bridge sources are in `cpp/`, build-time glue is in `build.rs`, and helper scripts live in `scripts/`. User-facing API examples are documented in `README.md` and `API.md`.

## Build, Test, and Development Commands
Use the checked-in Android build entrypoint:

```bash
ANDROID_NDK_HOME="/opt/homebrew/share/android-ndk" cargo ndk -t x86_64  build --release                      
```


## Coding Style & Naming Conventions
Follow standard Rust style: 4-space indentation, `snake_case` for modules/functions, `PascalCase` for types, and `SCREAMING_SNAKE_CASE` for constants. Keep modules focused; prefer adding a new file under `src/daemon/` or `src/ffi/` instead of growing `lib.rs`. Comments should explain design intent or FFI constraints, not restate obvious control flow.

## Testing Guidelines
Keep tests close to the code they cover with `#[cfg(test)] mod tests`. Existing tests in files such as `src/daemon/mp4.rs` and `src/ffi/runtime.rs` are the model: small unit tests, deterministic fixtures, and names that describe behavior. Add tests for parser changes, protocol framing, and MP4 processing regressions before merging.

## Commit & Pull Request Guidelines
Recent history uses short imperative subjects, with some Conventional Commit prefixes (`feat:`, `fix:`). Prefer `type: concise summary`, for example `fix: preserve login state after 2fa prompt`; avoid vague subjects like `update`. PRs should include the problem statement, the root cause, the exact verification commands you ran, and any API, cache, or rootfs impact. Attach request/response samples when an endpoint behavior changes.

## Security & Configuration Tips
Do not commit Apple credentials, tokens, or extracted native libraries. Keep `ANDROID_NDK_HOME` pointed at `/opt/homebrew/share/android-ndk` for local Android builds, and treat `rootfs/` paths and runtime cache contents as environment-specific artifacts.
