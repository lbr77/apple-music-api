# Repository Guidelines

## Project Structure & Module Organization
`src/bin/main.rs` starts the HTTP daemon, and `src/bin/wrapper.rs` launches the Android runtime inside `rootfs`. Shared logic lives under `src/`: `config/` handles CLI and device settings, `daemon/` serves HTTP endpoints and download flows, `ffi/` loads native Android symbols, `runtime/` owns session state, and `logging/` centralizes logs. Native bridge sources are in `cpp/`, build-time glue is in `build.rs`, helper scripts are in `scripts/`, and user-facing API docs live in `README.md` and `API.md`.

## Build, Test, and Development Commands
Use the canonical Android release build:

```bash
ANDROID_NDK_HOME="/opt/homebrew/share/android-ndk" cargo ndk -t x86_64 build --release
```

The checked-in shortcut is `./scripts/build-android.sh`. For Linux-only host debugging, use `cargo build --release`. Run `cargo test` on Linux, then verify style with `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`. Validate the packaged runtime with `docker build -t wrapper-rs .`, which matches the CI image build flow.

## Coding Style & Naming Conventions
Follow standard Rust conventions: 4-space indentation, `snake_case` for functions and modules, `PascalCase` for types, and `SCREAMING_SNAKE_CASE` for constants. Keep files focused; prefer adding a module under `src/daemon/` or `src/ffi/` over growing `src/lib.rs`. Comments should explain intent, FFI constraints, or platform trade-offs rather than narrating obvious control flow.

## Testing Guidelines
Keep tests close to the code they cover with `#[cfg(test)] mod tests`. Existing tests in `src/daemon/mp4.rs`, `src/config/device.rs`, and `src/ffi/runtime.rs` are the reference pattern: small unit tests, deterministic inputs, and behavior-based names. Run tests on Linux; macOS builds currently fail in `src/launcher.rs` because that module uses Linux-only `libc::unshare` and `CLONE_NEWPID`.

## Commit & Pull Request Guidelines
Recent commits use short imperative subjects, with some Conventional Commit prefixes such as `feat:` and `fix:`. Prefer `type: concise summary`, for example `fix: preserve login state after 2fa prompt`, and avoid vague subjects like `update`. PRs should explain the problem, root cause, verification commands, and any API, cache, or `rootfs` impact. Include request/response samples when endpoint behavior changes.

## Security & Configuration Tips
Do not commit Apple credentials, tokens, or extracted native libraries. Keep `ANDROID_NDK_HOME` pointed at `/opt/homebrew/share/android-ndk` for local Android builds, and treat `rootfs/` contents plus cache artifacts as machine-specific runtime data.
