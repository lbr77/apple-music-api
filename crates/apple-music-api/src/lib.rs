mod client;
mod error;
#[macro_use]
mod logging;

pub use client::{AppleApiClient, ArtistViewRequest, Artwork, SearchRequest, SongPlaybackMetadata};
pub use error::{ApiResult, AppleMusicApiError};
