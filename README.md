# wrapper-rs

Rust rewrite of the original `main.c` / `main.cpp` flow for `x86_64-linux-android`.

## Build

```bash
cd rust
cargo ndk -t x86_64 build --release
```

The release binary is `target/x86_64-linux-android/release/wrapper`.

## Run

```bash
orb sudo ./wrapper
```

## Control Protocol

The control server accepts one JSON request per TCP connection and returns one JSON response before closing the socket.

### Requests

```json
{"type":"login","username":"apple@example.com","password":"secret"}
{"type":"submit_2fa","code":"123456"}
{"type":"account_info"}
{"type":"query_m3u8","adam":"1440924808"}
{"type":"logout"}
{"type":"status"}
{"type":"refresh_lease"}
```

### Responses

```json
{"status":"ok","state":"logged_in"}
{"status":"ok","state":"logged_out"}
{"status":"ok","state":"logged_in","storefront_id":"...","dev_token":"...","music_token":"..."}
{"status":"ok","state":"logged_in","adam":"1440924808","url":"https://..."}
{"status":"need_2fa","state":"awaiting_2fa","message":"verification code required"}
{"status":"error","state":"logged_out","message":"login failed"}
```

If login needs 2FA, the server responds with `need_2fa` and closes that control connection.
The next control connection must send `{"type":"submit_2fa","code":"..."}` to resume the same native login flow.

`account_info` and `query_m3u8` are also part of the control plane now, so there is no separate account or m3u8 TCP listener anymore.

## Decrypt Flow

The decrypt plane stays on its own TCP port and keeps the legacy binary framing:

1. `u8 adam_len`
2. `adam_len` bytes of `adam`
3. `u8 uri_len`
4. `uri_len` bytes of `uri`
5. repeated `u32 native-endian sample_len + sample bytes`
6. `u32 == 0` terminates the stream

For each decrypt connection the runtime:

- reads the `(adam, uri)` context key once
- builds exactly one native decrypt context for that connection
- decrypts samples strictly in receive order
- writes each decrypted sample back immediately

The native decryptor keeps mutable state inside the context, so the Rust path mirrors the original C behavior instead of fanning a single stream out across multiple workers.
