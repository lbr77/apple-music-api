use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlRequest {
    Login { username: String, password: String },
    Submit2fa { code: String },
    AccountInfo,
    QueryM3u8 { adam: String },
    ResetLogin,
    Logout,
    Status,
    RefreshLease,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ControlResponse {
    pub status: &'static str,
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adam: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storefront_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub music_token: Option<String>,
}

impl ControlResponse {
    pub fn ok(state: &'static str) -> Self {
        Self {
            status: "ok",
            state,
            message: None,
            adam: None,
            url: None,
            storefront_id: None,
            dev_token: None,
            music_token: None,
        }
    }

    pub fn need_2fa(message: impl Into<String>) -> Self {
        Self {
            status: "need_2fa",
            state: "awaiting_2fa",
            message: Some(message.into()),
            adam: None,
            url: None,
            storefront_id: None,
            dev_token: None,
            music_token: None,
        }
    }

    pub fn error(state: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: "error",
            state,
            message: Some(message.into()),
            adam: None,
            url: None,
            storefront_id: None,
            dev_token: None,
            music_token: None,
        }
    }

    pub fn status(state: &'static str) -> Self {
        Self {
            status: "ok",
            state,
            message: None,
            adam: None,
            url: None,
            storefront_id: None,
            dev_token: None,
            music_token: None,
        }
    }

    pub fn account_info(
        state: &'static str,
        storefront_id: String,
        dev_token: String,
        music_token: String,
    ) -> Self {
        Self {
            status: "ok",
            state,
            message: None,
            adam: None,
            url: None,
            storefront_id: Some(storefront_id),
            dev_token: Some(dev_token),
            music_token: Some(music_token),
        }
    }

    pub fn query_m3u8(state: &'static str, adam: String, url: String) -> Self {
        Self {
            status: "ok",
            state,
            message: None,
            adam: Some(adam),
            url: Some(url),
            storefront_id: None,
            dev_token: None,
            music_token: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ControlRequest, ControlResponse};

    #[test]
    fn request_roundtrip_uses_type_tag() {
        let json = serde_json::to_string(&ControlRequest::Login {
            username: "user".into(),
            password: "pw".into(),
        })
        .expect("serialize request");
        assert!(json.contains("\"type\":\"login\""));
    }

    #[test]
    fn response_roundtrip_has_state() {
        let json = serde_json::to_string(&ControlResponse::need_2fa("verification code required"))
            .expect("serialize response");
        assert!(json.contains("\"status\":\"need_2fa\""));
        assert!(json.contains("\"state\":\"awaiting_2fa\""));
    }

    #[test]
    fn account_info_response_serializes_payload_fields() {
        let json = serde_json::to_string(&ControlResponse::account_info(
            "logged_in",
            "storefront".into(),
            "dev".into(),
            "music".into(),
        ))
        .expect("serialize response");
        assert!(json.contains("\"storefront_id\":\"storefront\""));
        assert!(json.contains("\"dev_token\":\"dev\""));
        assert!(json.contains("\"music_token\":\"music\""));
    }
}
