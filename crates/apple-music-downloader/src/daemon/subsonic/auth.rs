use std::sync::Arc;

use axum::extract::{Request, State};
use axum::response::Response;

use super::error::SubsonicError;
use super::{AuthQuery, DaemonContext, ResponseFormat};

pub(super) async fn require_subsonic_auth(
    State(context): State<Arc<DaemonContext>>,
    request: Request,
    next: axum::middleware::Next,
) -> Result<Response, SubsonicError> {
    let query = parse_auth_query(request.uri().query())?;
    validate_auth(&context, &query)?;
    Ok(next.run(request).await)
}

pub(super) fn parse_auth_query(raw_query: Option<&str>) -> Result<AuthQuery, SubsonicError> {
    let format = raw_query
        .and_then(|query| serde_urlencoded::from_str::<AuthQuery>(query).ok())
        .map(|query| query.f.clone())
        .and_then(|value| ResponseFormat::from_query(value.as_deref()).ok())
        .unwrap_or(ResponseFormat::Xml);
    serde_urlencoded::from_str(raw_query.unwrap_or("")).map_err(|error| {
        SubsonicError::required_parameter(format, format!("invalid query string: {error}"))
    })
}

pub(super) fn response_format(query: &AuthQuery) -> Result<ResponseFormat, SubsonicError> {
    ResponseFormat::from_query(query.f.as_deref())
        .map_err(|message| SubsonicError::required_parameter(ResponseFormat::Xml, message))
}

pub(super) fn validate_auth(
    context: &DaemonContext,
    query: &AuthQuery,
) -> Result<(), SubsonicError> {
    validate_auth_credentials(
        context.subsonic_username(),
        context.subsonic_password(),
        query,
    )
}

pub(super) fn validate_auth_credentials(
    expected_username: &str,
    expected_password: &str,
    query: &AuthQuery,
) -> Result<(), SubsonicError> {
    let format = response_format(query)?;
    let username = query
        .u
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SubsonicError::required_parameter(format, "u is required"))?;
    if query
        .v
        .as_deref()
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return Err(SubsonicError::required_parameter(format, "v is required"));
    }
    if query
        .c
        .as_deref()
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return Err(SubsonicError::required_parameter(format, "c is required"));
    }
    if username != expected_username {
        return Err(SubsonicError::authentication(
            format,
            "wrong username or password",
        ));
    }

    let authenticated = if let Some(password) = query.p.as_deref() {
        match decode_password(password) {
            Ok(decoded) => decoded == expected_password,
            Err(error) => return Err(error),
        }
    } else if let (Some(token), Some(salt)) = (query.t.as_deref(), query.s.as_deref()) {
        let digest = md5::compute(format!("{expected_password}{salt}"));
        format!("{:x}", digest) == token.to_ascii_lowercase()
    } else {
        return Err(SubsonicError::required_parameter(
            format,
            "either p or t+s is required",
        ));
    };

    if authenticated {
        Ok(())
    } else {
        Err(SubsonicError::authentication(
            format,
            "wrong username or password",
        ))
    }
}

fn decode_password(password: &str) -> Result<&str, SubsonicError> {
    if let Some(encoded) = password.strip_prefix("enc:") {
        let decoded = hex::decode(encoded).map_err(|_| {
            SubsonicError::authentication(ResponseFormat::Xml, "invalid encoded password")
        })?;
        let text = String::from_utf8(decoded).map_err(|_| {
            SubsonicError::authentication(ResponseFormat::Xml, "invalid encoded password")
        })?;
        Ok(Box::leak(text.into_boxed_str()))
    } else {
        Ok(password)
    }
}
