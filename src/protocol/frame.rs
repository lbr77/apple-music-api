use std::io::{Read, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{AppError, AppResult};

pub fn write_frame<T: Serialize>(writer: &mut impl Write, value: &T) -> AppResult<()> {
    let payload = serde_json::to_vec(value)?;
    let len = u32::try_from(payload.len())
        .map_err(|_| AppError::Protocol("frame payload exceeds u32::MAX".into()))?;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(&payload)?;
    Ok(())
}

pub fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> AppResult<T> {
    let mut len_buf = [0_u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut payload = vec![0_u8; len];
    reader.read_exact(&mut payload)?;
    Ok(serde_json::from_slice(&payload)?)
}
