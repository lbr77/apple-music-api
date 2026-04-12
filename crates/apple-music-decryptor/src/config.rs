use std::path::PathBuf;

use crate::device::DeviceInfo;

#[derive(Clone, Debug)]
pub struct NativePlatformConfig {
    pub proxy: Option<String>,
    pub base_dir: PathBuf,
    pub library_dir: PathBuf,
    pub device_info: DeviceInfo,
}

#[derive(Clone, Debug)]
pub struct DownloadConfig {
    pub proxy: Option<String>,
    pub cache_dir: PathBuf,
}
