mod config;
mod device;
mod download;
mod error;
pub mod ffi;
mod logging;
mod mp4;
mod session;

pub use config::{DownloadConfig, NativePlatformConfig};
pub use device::DeviceInfo;
pub use download::{
    ArtworkDescriptor, BinaryHealth, PlaybackOutput, PlaybackRequest, PlaybackTrackMetadata,
    ToolHealthReport, download_playback, tool_health_report,
};
pub use error::{AppResult, AppleMusicDecryptorError};
pub use ffi::{
    AccountProfile, ContextKey, LoginAttempt, LoginWaitState, NativePlatform, NativeSession,
    PContextHandle, install_android_log_shim,
};
pub use session::SessionRuntime;
