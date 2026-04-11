use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::ffi::{ContextKey, NativePlatform};
use crate::runtime::AppState;

pub fn run_decrypt_server(
    config: AppConfig,
    _platform: Arc<NativePlatform>,
    state: Arc<AppState>,
) -> AppResult<()> {
    let listener = TcpListener::bind(config.decrypt_addr())?;
    crate::app_info!(
        "server::decrypt",
        "listening for decrypt requests on {}",
        config.decrypt_addr()
    );
    for stream in listener.incoming() {
        let stream = stream?;
        let peer = peer_label(&stream);
        let state = Arc::clone(&state);
        thread::Builder::new()
            .name("decrypt-conn".into())
            .spawn(move || {
                crate::app_info!("server::decrypt", "accepted decrypt connection from {peer}");
                if let Err(error) = handle_decrypt_connection(stream, state) {
                    crate::app_error!(
                        "server::decrypt",
                        "decrypt connection {peer} failed: {error}"
                    );
                }
            })?;
    }
    Ok(())
}

fn handle_decrypt_connection(mut stream: TcpStream, state: Arc<AppState>) -> AppResult<()> {
    let peer = peer_label(&stream);
    let session = state.session().ok_or(AppError::NoActiveSession)?;
    crate::app_info!(
        "server::decrypt",
        "decrypt connection {peer} attached to active session"
    );
    let key = read_context_key(&mut stream)?;
    crate::app_info!(
        "server::decrypt",
        "decrypt context selected by {peer}: adam={}, uri={}",
        key.adam,
        key.uri,
    );
    let native = session.native();
    let mut context = native.build_context(&key)?;
    let mut sequence = 0_u64;

    loop {
        let Some(sample) = read_sample(&mut stream)? else {
            crate::app_info!(
                "server::decrypt",
                "decrypt connection {peer} reached end-of-stream after {sequence} samples"
            );
            break;
        };
        crate::app_info!(
            "server::decrypt",
            "decrypting sample from {peer}: sequence={sequence}, bytes={}",
            sample.len(),
        );
        let sample = native.decrypt_sample(&mut context, sample)?;
        crate::app_info!(
            "server::decrypt",
            "writing decrypted sample to {peer}: sequence={sequence}, bytes={}",
            sample.len(),
        );
        stream.write_all(&sample)?;
        sequence += 1;
    }
    crate::app_info!(
        "server::decrypt",
        "decrypt connection {peer} drained successfully: total_samples={sequence}"
    );
    Ok(())
}

fn read_context_key(stream: &mut TcpStream) -> AppResult<ContextKey> {
    let adam = read_string(stream)?;
    let uri = read_string(stream)?;
    Ok(ContextKey { adam, uri })
}

fn read_sample(stream: &mut TcpStream) -> AppResult<Option<Vec<u8>>> {
    let mut size_buf = [0_u8; 4];
    match stream.read_exact(&mut size_buf) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let size = u32::from_ne_bytes(size_buf);
    if size == 0 {
        return Ok(None);
    }

    let mut sample = vec![0_u8; size as usize];
    stream.read_exact(&mut sample)?;
    Ok(Some(sample))
}

fn read_string(stream: &mut TcpStream) -> AppResult<String> {
    let mut len = [0_u8; 1];
    stream.read_exact(&mut len)?;
    if len[0] == 0 {
        return Err(AppError::Protocol(
            "zero-length decrypt field is invalid".into(),
        ));
    }
    let mut value = vec![0_u8; len[0] as usize];
    stream.read_exact(&mut value)?;
    String::from_utf8(value).map_err(|error| AppError::Protocol(error.to_string()))
}

fn peer_label(stream: &TcpStream) -> String {
    stream
        .peer_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| "<unknown-peer>".into())
}
