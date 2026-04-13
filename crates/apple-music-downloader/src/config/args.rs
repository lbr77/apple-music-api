use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::{env, ffi::OsString};

use apple_music_decryptor::{DeviceInfo, DownloadConfig, NativePlatformConfig};
use clap::Parser;

use crate::error::{AppError, AppResult};

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub host: IpAddr,
    pub daemon_port: u16,
    pub api_token: String,
    pub media_user_token: Option<String>,
    pub subsonic_username: String,
    pub subsonic_password: String,
    pub proxy: Option<String>,
    pub base_dir: PathBuf,
    pub library_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub storefront: String,
    pub language: String,
    pub device_info: DeviceInfo,
    pub decrypt_workers: usize,
    pub decrypt_inflight: usize,
}

#[derive(Debug, Parser)]
#[command(name = "wrapper-rust", about = "Rust rewrite of wrapper main runtime")]
struct Args {
    #[arg(short = 'H', long = "host", default_value = "127.0.0.1")]
    host: IpAddr,
    #[arg(long = "daemon-port", default_value_t = 8080)]
    daemon_port: u16,
    #[arg(long = "api-token")]
    api_token: String,
    #[arg(long = "media-user-token")]
    media_user_token: Option<String>,
    #[arg(short = 'P', long = "proxy")]
    proxy: Option<String>,
    #[arg(
        short = 'B',
        long = "base-dir",
        default_value = "/data/data/com.apple.android.music/files"
    )]
    base_dir: PathBuf,
    #[arg(long = "lib-dir")]
    library_dir: Option<PathBuf>,
    #[arg(long = "cache-dir", default_value = "cache")]
    cache_dir: PathBuf,
    #[arg(long = "storefront", default_value = "us")]
    storefront: String,
    #[arg(long = "language", default_value = "")]
    language: String,
    #[arg(
        short = 'I',
        long = "device-info",
        default_value = "Music/4.9/Android/10/Samsung S9/7663313/en-US/en-US/dc28071e981c439e"
    )]
    device_info: String,
    #[arg(long = "decrypt-workers")]
    decrypt_workers: Option<usize>,
    #[arg(long = "decrypt-inflight")]
    decrypt_inflight: Option<usize>,
}

impl AppConfig {
    pub fn parse() -> AppResult<Self> {
        let args = Args::parse();
        let device_info = DeviceInfo::parse(&args.device_info)?;
        let decrypt_workers = args
            .decrypt_workers
            .unwrap_or_else(|| num_cpus::get().clamp(2, 8));
        let decrypt_inflight = args
            .decrypt_inflight
            .unwrap_or_else(|| decrypt_workers.saturating_mul(2).max(2));

        Ok(Self {
            host: args.host,
            daemon_port: args.daemon_port,
            api_token: normalize_api_token(args.api_token)?,
            media_user_token: normalize_optional_token(
                args.media_user_token
                    .or_else(|| env::var("MEDIA_USER_TOKEN").ok()),
            ),
            subsonic_username: normalize_subsonic_username(read_env_with_default(
                "SUBSONIC_USERNAME",
                "admin",
            ))?,
            subsonic_password: normalize_subsonic_password(read_env_with_default(
                "SUBSONIC_PASSWORD",
                "admin123",
            ))?,
            proxy: args.proxy,
            base_dir: args.base_dir,
            library_dir: resolve_library_dir(args.library_dir)?,
            cache_dir: args.cache_dir,
            storefront: normalize_storefront(args.storefront),
            language: args.language,
            device_info,
            decrypt_workers,
            decrypt_inflight,
        })
    }

    pub fn daemon_addr(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.daemon_port)
    }

    pub fn native_platform_config(&self) -> NativePlatformConfig {
        NativePlatformConfig {
            proxy: self.proxy.clone(),
            base_dir: self.base_dir.clone(),
            library_dir: self.library_dir.clone(),
            device_info: self.device_info.clone(),
        }
    }

    pub fn download_config(&self) -> DownloadConfig {
        DownloadConfig {
            proxy: self.proxy.clone(),
            cache_dir: self.cache_dir.clone(),
        }
    }
}

fn resolve_library_dir(override_dir: Option<PathBuf>) -> AppResult<PathBuf> {
    if let Some(path) = override_dir {
        return Ok(path);
    }

    let candidates = [
        Path::new("/system/lib64"),
        Path::new("rootfs/system/lib64"),
        Path::new("./rootfs/system/lib64"),
    ];

    candidates
        .iter()
        .find(|path| path.exists())
        .map(|path| path.to_path_buf())
        .ok_or_else(|| {
            AppError::Message(
                "unable to resolve rootfs library directory; pass --lib-dir explicitly".into(),
            )
        })
}

fn normalize_storefront(storefront: String) -> String {
    let storefront = storefront.trim().to_ascii_lowercase();
    if storefront.len() == 2 {
        storefront
    } else {
        "us".into()
    }
}

fn normalize_api_token(api_token: String) -> AppResult<String> {
    let api_token = api_token.trim().to_owned();
    if api_token.is_empty() {
        Err(AppError::Message(
            "api token cannot be empty; pass --api-token".into(),
        ))
    } else {
        Ok(api_token)
    }
}

fn normalize_optional_token(token: Option<String>) -> Option<String> {
    token.and_then(|value| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    })
}

fn normalize_subsonic_username(username: String) -> AppResult<String> {
    let username = username.trim().to_owned();
    if username.is_empty() {
        Err(AppError::Message(
            "subsonic username cannot be empty; set SUBSONIC_USERNAME".into(),
        ))
    } else {
        Ok(username)
    }
}

fn normalize_subsonic_password(password: String) -> AppResult<String> {
    let password = password.trim().to_owned();
    if password.is_empty() {
        Err(AppError::Message(
            "subsonic password cannot be empty; set SUBSONIC_PASSWORD".into(),
        ))
    } else {
        Ok(password)
    }
}

fn read_env_with_default(name: &str, default_value: &str) -> String {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsString::from(default_value))
        .to_string_lossy()
        .into_owned()
}
