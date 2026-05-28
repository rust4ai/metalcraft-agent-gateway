use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};

/// Middleware: verify `Authorization: Bearer <AGENT_GATEWAY_API_KEY>`.
///
/// The key is guaranteed to be set at boot (main.rs validates this).
pub async fn require_api_key(req: Request, next: Next) -> Result<Response, StatusCode> {
    let expected = std::env::var("AGENT_GATEWAY_API_KEY").unwrap_or_default();

    let header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    if let Some(token) = header.strip_prefix("Bearer ") {
        if token == expected {
            return Ok(next.run(req).await);
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}
