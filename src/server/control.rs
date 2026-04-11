use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use crate::config::AppConfig;
use crate::error::AppResult;
use crate::ffi::{LoginAttempt, LoginWaitState, NativePlatform};
use crate::protocol::{ControlRequest, ControlResponse, read_frame, write_frame};
use crate::runtime::{AppState, SessionRuntime};

pub fn run_control_server(
    config: AppConfig,
    platform: Arc<NativePlatform>,
    state: Arc<AppState>,
) -> AppResult<()> {
    let listener = TcpListener::bind(config.control_addr())?;
    crate::app_info!(
        "server::control",
        "listening for control commands on {}",
        config.control_addr()
    );
    for stream in listener.incoming() {
        let stream = stream?;
        let peer = peer_label(&stream);
        let config = config.clone();
        let platform = Arc::clone(&platform);
        let state = Arc::clone(&state);
        thread::Builder::new()
            .name("control-conn".into())
            .spawn(move || {
                crate::app_info!("server::control", "accepted control connection from {peer}");
                if let Err(error) = handle_control_connection(stream, config, platform, state) {
                    crate::app_error!(
                        "server::control",
                        "control connection {peer} failed: {error}"
                    );
                }
            })?;
    }
    Ok(())
}

fn handle_control_connection(
    mut stream: TcpStream,
    _config: AppConfig,
    platform: Arc<NativePlatform>,
    state: Arc<AppState>,
) -> AppResult<()> {
    let peer = peer_label(&stream);
    let request = read_frame::<ControlRequest>(&mut stream)?;
    crate::app_info!(
        "server::control",
        "received control request from {peer}: {} (state={})",
        request_name(&request),
        state_name(&state),
    );

    match request {
        ControlRequest::Login { username, password } => {
            if state.session().is_some() {
                crate::app_warn!(
                    "server::control",
                    "rejecting login from {peer}: session already active"
                );
                write_frame(
                    &mut stream,
                    &ControlResponse::error("logged_in", "logout before starting a new login"),
                )?;
                return Ok(());
            }
            if state.pending_login().is_some() {
                crate::app_warn!(
                    "server::control",
                    "rejecting login from {peer}: another login is waiting for 2FA"
                );
                write_frame(
                    &mut stream,
                    &ControlResponse::error(
                        "awaiting_2fa",
                        "a previous login is still waiting for 2FA",
                    ),
                )?;
                return Ok(());
            }

            let attempt = LoginAttempt::new(username, password);
            let login_attempt = Arc::clone(&attempt);
            let login_platform = Arc::clone(&platform);
            crate::app_info!("server::control", "spawning native login worker for {peer}");
            thread::Builder::new()
                .name("native-login".into())
                .spawn(move || {
                    crate::app_info!("server::control", "native login worker started");
                    let result = login_platform.login(Arc::clone(&login_attempt));
                    login_attempt.finish(result);
                })?;

            match attempt.wait_for_initial_state() {
                LoginWaitState::NeedTwoFactor => {
                    state.set_pending_login(attempt);
                    crate::app_warn!(
                        "server::control",
                        "login from {peer} now awaiting 2FA on a future connection"
                    );
                    write_frame(
                        &mut stream,
                        &ControlResponse::need_2fa("verification code required"),
                    )?;
                }
                LoginWaitState::Completed(result) => match result {
                    Ok(session) => {
                        crate::app_info!("server::control", "login from {peer} completed");
                        let session = SessionRuntime::new(session)?;
                        state.replace_session(Arc::new(session));
                        write_frame(&mut stream, &ControlResponse::ok("logged_in"))?;
                    }
                    Err(error) => {
                        crate::app_error!(
                            "server::control",
                            "login from {peer} failed before session creation: {error}"
                        );
                        write_frame(
                            &mut stream,
                            &ControlResponse::error("logged_out", error.to_string()),
                        )?;
                    }
                },
            }
        }
        ControlRequest::Logout => {
            if state.pending_login().is_some() {
                crate::app_warn!(
                    "server::control",
                    "rejecting logout from {peer}: login still waiting for 2FA"
                );
                write_frame(
                    &mut stream,
                    &ControlResponse::error(
                        "awaiting_2fa",
                        "cannot logout while a login is waiting for 2FA",
                    ),
                )?;
                return Ok(());
            }
            if let Some(session) = state.clear_session() {
                crate::app_info!("server::control", "processing logout from {peer}");
                session.native().logout()?;
            }
            write_frame(&mut stream, &ControlResponse::ok("logged_out"))?;
            crate::app_info!("server::control", "logout request from {peer} completed");
        }
        ControlRequest::ResetLogin => {
            if state.session().is_some() {
                crate::app_warn!(
                    "server::control",
                    "rejecting reset_login from {peer}: session already active"
                );
                write_frame(
                    &mut stream,
                    &ControlResponse::error("logged_in", "use logout to clear the active session"),
                )?;
                return Ok(());
            }

            let pending = state.clear_pending_login();
            if let Some(attempt) = pending {
                crate::app_warn!(
                    "server::control",
                    "reset_login from {peer} canceled the pending 2FA login"
                );
                attempt.cancel("login reset by control command");
            } else {
                crate::app_info!(
                    "server::control",
                    "reset_login from {peer} found no pending login"
                );
            }
            write_frame(&mut stream, &ControlResponse::ok("logged_out"))?;
            crate::app_info!(
                "server::control",
                "reset_login request from {peer} completed"
            );
        }
        ControlRequest::Submit2fa { code } => {
            let Some(attempt) = state.take_pending_login() else {
                crate::app_warn!(
                    "server::control",
                    "rejecting submit_2fa from {peer}: no pending login"
                );
                write_frame(
                    &mut stream,
                    &ControlResponse::error(
                        "logged_out",
                        "submit_2fa is only valid after login returns need_2fa",
                    ),
                )?;
                return Ok(());
            };

            crate::app_info!(
                "server::control",
                "received submit_2fa from {peer}: code_len={}",
                code.len(),
            );
            attempt.submit_two_factor(code)?;
            match attempt.wait_for_completion() {
                Ok(session) => {
                    let session = SessionRuntime::new(session)?;
                    state.replace_session(Arc::new(session));
                    write_frame(&mut stream, &ControlResponse::ok("logged_in"))?;
                    crate::app_info!("server::control", "2FA login from {peer} completed");
                }
                Err(error) => {
                    crate::app_error!(
                        "server::control",
                        "2FA login from {peer} failed before session creation: {error}"
                    );
                    write_frame(
                        &mut stream,
                        &ControlResponse::error("logged_out", error.to_string()),
                    )?;
                }
            }
        }
        ControlRequest::Status => {
            let current_state = state_name(&state);
            crate::app_info!(
                "server::control",
                "status request from {peer}: returning {current_state}"
            );
            write_frame(&mut stream, &ControlResponse::status(current_state))?;
        }
        ControlRequest::AccountInfo => {
            let Some(session) = state.session() else {
                crate::app_warn!(
                    "server::control",
                    "rejecting account_info from {peer}: no active session"
                );
                write_frame(
                    &mut stream,
                    &ControlResponse::error("logged_out", "no active session"),
                )?;
                return Ok(());
            };
            let profile = session.account_profile();
            crate::app_info!(
                "server::control",
                "returning account_info to {peer}: storefront_bytes={}, dev_token_bytes={}, music_token_bytes={}",
                profile.storefront_id.len(),
                profile.dev_token.len(),
                profile.music_token.len(),
            );
            write_frame(
                &mut stream,
                &ControlResponse::account_info(
                    "logged_in",
                    profile.storefront_id.clone(),
                    profile.dev_token.clone(),
                    profile.music_token.clone(),
                ),
            )?;
        }
        ControlRequest::QueryM3u8 { adam } => {
            let Some(session) = state.session() else {
                crate::app_warn!(
                    "server::control",
                    "rejecting query_m3u8 from {peer}: no active session"
                );
                write_frame(
                    &mut stream,
                    &ControlResponse::error("logged_out", "no active session"),
                )?;
                return Ok(());
            };
            let adam_id = match adam.parse::<u64>() {
                Ok(value) => value,
                Err(error) => {
                    crate::app_warn!(
                        "server::control",
                        "rejecting query_m3u8 from {peer}: invalid adam={adam}, error={error}"
                    );
                    write_frame(
                        &mut stream,
                        &ControlResponse::error("logged_in", format!("invalid adam: {error}")),
                    )?;
                    return Ok(());
                }
            };
            crate::app_info!(
                "server::control",
                "processing query_m3u8 from {peer}: adam={adam_id}"
            );
            match session.resolve_m3u8_url(adam_id) {
                Ok(url) => {
                    write_frame(
                        &mut stream,
                        &ControlResponse::query_m3u8("logged_in", adam, url),
                    )?;
                }
                Err(error) => {
                    crate::app_error!(
                        "server::control",
                        "query_m3u8 from {peer} failed: adam={adam_id}, error={error}"
                    );
                    write_frame(
                        &mut stream,
                        &ControlResponse::error("logged_in", error.to_string()),
                    )?;
                }
            }
        }
        ControlRequest::RefreshLease => {
            let Some(session) = state.session() else {
                crate::app_warn!(
                    "server::control",
                    "rejecting refresh_lease from {peer}: no active session"
                );
                write_frame(
                    &mut stream,
                    &ControlResponse::error("logged_out", "no active session to refresh"),
                )?;
                return Ok(());
            };
            crate::app_info!("server::control", "refresh_lease requested from {peer}");
            session.refresh_lease()?;
            write_frame(&mut stream, &ControlResponse::ok("logged_in"))?;
            crate::app_info!("server::control", "refresh_lease from {peer} completed");
        }
    }
    crate::app_info!("server::control", "closing control connection from {peer}");
    Ok(())
}

fn peer_label(stream: &TcpStream) -> String {
    stream
        .peer_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| "<unknown-peer>".into())
}

fn request_name(request: &ControlRequest) -> &'static str {
    match request {
        ControlRequest::Login { .. } => "login",
        ControlRequest::ResetLogin => "reset_login",
        ControlRequest::Logout => "logout",
        ControlRequest::Submit2fa { .. } => "submit_2fa",
        ControlRequest::Status => "status",
        ControlRequest::AccountInfo => "account_info",
        ControlRequest::QueryM3u8 { .. } => "query_m3u8",
        ControlRequest::RefreshLease => "refresh_lease",
    }
}

fn state_name(state: &AppState) -> &'static str {
    if state.pending_login().is_some() {
        "awaiting_2fa"
    } else if state.session().is_some() {
        "logged_in"
    } else {
        "logged_out"
    }
}
