mod client;
mod error;

pub use client::{AppleApiClient, ArtistViewRequest, Artwork, SearchRequest, SongPlaybackMetadata};
pub use error::{ApiResult, AppleMusicApiError};
