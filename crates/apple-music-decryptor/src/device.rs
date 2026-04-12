use crate::error::{AppResult, AppleMusicDecryptorError as AppError};

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub client_identifier: String,
    pub version_identifier: String,
    pub platform_identifier: String,
    pub product_version: String,
    pub device_model: String,
    pub build_version: String,
    pub locale_identifier: String,
    pub language_identifier: String,
    pub android_id: String,
}

impl DeviceInfo {
    pub fn parse(value: &str) -> AppResult<Self> {
        let parts: Vec<_> = value.split('/').collect();
        if parts.len() != 9 {
            return Err(AppError::InvalidDeviceInfo(format!(
                "expected 9 slash-separated fields, got {}",
                parts.len()
            )));
        }

        Ok(Self {
            client_identifier: parts[0].to_owned(),
            version_identifier: parts[1].to_owned(),
            platform_identifier: parts[2].to_owned(),
            product_version: parts[3].to_owned(),
            device_model: parts[4].to_owned(),
            build_version: parts[5].to_owned(),
            locale_identifier: parts[6].to_owned(),
            language_identifier: parts[7].to_owned(),
            android_id: parts[8].to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::DeviceInfo;

    #[test]
    fn parses_expected_shape() {
        let info = DeviceInfo::parse(
            "Music/4.9/Android/10/Samsung S9/7663313/en-US/en-US/dc28071e981c439e",
        )
        .expect("device info should parse");
        assert_eq!(info.client_identifier, "Music");
        assert_eq!(info.android_id, "dc28071e981c439e");
    }

    #[test]
    fn rejects_short_shape() {
        assert!(DeviceInfo::parse("Music/4.9/Android").is_err());
    }
}
