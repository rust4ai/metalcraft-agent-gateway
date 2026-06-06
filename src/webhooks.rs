use std::collections::BTreeMap;
use std::sync::Arc;

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::post,
    Json,
};
use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::Sha256;
use serde_json::{json, Value};

use crate::AppState;
use crate::events::{EventAuthor, GatewayEvent};

type S = Arc<AppState>;

pub fn router() -> Router<S> {
    Router::new()
        .route("/slack", post(slack_webhook))
        .route("/github", post(github_webhook))
        .route("/twilio", post(twilio_webhook))
}

// ── Slack Webhook ───────────────────────────────────────────────────────

async fn slack_webhook(
    State(state): State<S>,
    headers: HeaderMap,
    raw_body: Bytes,
) -> impl IntoResponse {
    // Verify Slack signature if signing secret is configured
    if let Ok(signing_secret) = std::env::var("SLACK_SIGNING_SECRET") {
        if !signing_secret.is_empty() {
            let timestamp = headers
                .get("x-slack-request-timestamp")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            let signature = headers
                .get("x-slack-signature")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();

            if !verify_slack_signature(&signing_secret, timestamp, &raw_body, signature) {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "Invalid signature" })),
                )
                    .into_response();
            }
        }
    }

    let body: Value = match serde_json::from_slice(&raw_body) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Invalid JSON" })),
            )
                .into_response();
        }
    };

    // Handle url_verification challenge
    if body.get("type").and_then(|t| t.as_str()) == Some("url_verification") {
        let challenge = body
            .get("challenge")
            .and_then(|c| c.as_str())
            .unwrap_or_default();
        return (StatusCode::OK, Json(json!({ "challenge": challenge }))).into_response();
    }

    // Process event callback
    if let Some(event) = body.get("event") {
        let slack_type = event
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("unknown");

        let event_type = match slack_type {
            "message" => "message_create",
            "reaction_added" => "reaction_add",
            "reaction_removed" => "reaction_remove",
            "app_mention" => "app_mention",
            other => other,
        };

        let channel_id = event.get("channel").and_then(|c| c.as_str()).map(String::from);
        let text = event.get("text").and_then(|t| t.as_str()).map(String::from);
        let user_id = event.get("user").and_then(|u| u.as_str()).map(String::from);

        let author = user_id.map(|uid| EventAuthor {
            id: uid.clone(),
            username: uid,
            display_name: None,
            is_bot: event.get("bot_id").is_some(),
        });

        // Convert Slack epoch timestamp to RFC 3339
        let timestamp = event
            .get("ts")
            .and_then(|t| t.as_str())
            .and_then(|ts| {
                ts.split('.').next()
                    .and_then(|secs| secs.parse::<i64>().ok())
                    .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
                    .map(|dt| dt.to_rfc3339())
            })
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

        let gateway_event = GatewayEvent {
            id: uuid::Uuid::new_v4().to_string(),
            platform: "slack".into(),
            event_type: event_type.into(),
            channel_id,
            author,
            content: text,
            timestamp,
            raw: body.clone(),
        };

        let _ = state.event_tx.send(gateway_event);
    }

    (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
}

// ── GitHub Webhook ──────────────────────────────────────────────────────

async fn github_webhook(
    State(state): State<S>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // Verify HMAC signature if secret is configured
    if let Ok(secret) = std::env::var("GITHUB_WEBHOOK_SECRET") {
        if !secret.is_empty() {
            let signature = headers
                .get("x-hub-signature-256")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();

            if !verify_github_signature(&secret, &body, signature) {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "Invalid signature" })),
                );
            }
        }
    }

    let gh_event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    let payload: Value = serde_json::from_slice(&body).unwrap_or_default();

    let action = payload
        .get("action")
        .and_then(|a| a.as_str())
        .unwrap_or_default();

    let event_type = match (gh_event, action) {
        ("pull_request", "opened") => "pr_opened",
        ("pull_request", "closed") => {
            if payload
                .get("pull_request")
                .and_then(|pr| pr.get("merged"))
                .and_then(|m| m.as_bool())
                .unwrap_or(false)
            {
                "pr_merged"
            } else {
                "pr_closed"
            }
        }
        ("pull_request", action) => action,
        ("issue_comment", _) => "issue_comment",
        ("push", _) => "push",
        ("issues", "opened") => "issue_opened",
        ("issues", "closed") => "issue_closed",
        (other, _) => other,
    };

    // Extract author info from sender
    let author = payload.get("sender").map(|sender| EventAuthor {
        id: sender
            .get("id")
            .and_then(|id| id.as_u64())
            .map(|id| id.to_string())
            .unwrap_or_default(),
        username: sender
            .get("login")
            .and_then(|l| l.as_str())
            .unwrap_or_default()
            .to_string(),
        display_name: None,
        is_bot: sender
            .get("type")
            .and_then(|t| t.as_str())
            .map(|t| t == "Bot")
            .unwrap_or(false),
    });

    // Extract repo as channel_id
    let channel_id = payload
        .get("repository")
        .and_then(|r| r.get("full_name"))
        .and_then(|n| n.as_str())
        .map(String::from);

    let gateway_event = GatewayEvent {
        id: uuid::Uuid::new_v4().to_string(),
        platform: "github".into(),
        event_type: event_type.into(),
        channel_id,
        author,
        content: None,
        timestamp: chrono::Utc::now().to_rfc3339(),
        raw: payload,
    };

    let _ = state.event_tx.send(gateway_event);

    (StatusCode::OK, Json(json!({ "ok": true })))
}

// ── Twilio (WhatsApp) Webhook ─────────────────────────────────────────────

/// Inbound WhatsApp messages arrive here as `application/x-www-form-urlencoded`
/// POSTs from Twilio. Optionally verifies the `X-Twilio-Signature` header (when
/// `TWILIO_AUTH_TOKEN` + `TWILIO_WEBHOOK_URL` are set), then enforces the
/// `TWILIO_ALLOWED_NUMBERS` phone allowlist before broadcasting the event.
async fn twilio_webhook(
    State(state): State<S>,
    headers: HeaderMap,
    raw_body: Bytes,
) -> impl IntoResponse {
    // Parse the form body. A BTreeMap keeps keys sorted, which is exactly what
    // Twilio's signature scheme requires.
    let params: BTreeMap<String, String> = form_urlencoded::parse(&raw_body)
        .into_owned()
        .collect();

    // Verify the Twilio signature when configured. Twilio signs over the public
    // webhook URL it POSTed to, so we need that exact URL (TWILIO_WEBHOOK_URL),
    // which the gateway can't infer from behind the reverse proxy.
    if let (Ok(token), Ok(public_url)) = (
        std::env::var("TWILIO_AUTH_TOKEN"),
        std::env::var("TWILIO_WEBHOOK_URL"),
    ) {
        if !token.is_empty() && !public_url.is_empty() {
            let signature = headers
                .get("x-twilio-signature")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            if !verify_twilio_signature(&token, &public_url, &params, signature) {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "Invalid signature" })),
                )
                    .into_response();
            }
        }
    }

    let from = strip_whatsapp_prefix(params.get("From").map(String::as_str).unwrap_or_default());
    let content = params.get("Body").cloned().filter(|s| !s.is_empty());
    let profile_name = params.get("ProfileName").cloned().filter(|s| !s.is_empty());

    // Phone-number allowlist. When TWILIO_ALLOWED_NUMBERS is set, only those
    // numbers may reach the agent; everyone else is acknowledged but dropped.
    if let Some(allowed) = allowed_numbers() {
        if !allowed.iter().any(|n| n == &from) {
            tracing::warn!("WhatsApp message from non-allowlisted number {from} ignored");
            return twiml_ok();
        }
    }

    let author = EventAuthor {
        id: from.clone(),
        username: from.clone(),
        display_name: profile_name,
        is_bot: false,
    };

    let gateway_event = GatewayEvent {
        id: uuid::Uuid::new_v4().to_string(),
        platform: "whatsapp".into(),
        event_type: "message_create".into(),
        // The sender's number is also the reply target (send back via platform=whatsapp).
        channel_id: Some(from),
        author: Some(author),
        content,
        timestamp: chrono::Utc::now().to_rfc3339(),
        raw: serde_json::to_value(&params).unwrap_or(Value::Null),
    };

    let _ = state.event_tx.send(gateway_event);

    twiml_ok()
}

/// Empty TwiML 200 response — acknowledges the message without an auto-reply
/// (the agent replies asynchronously via the outbound API).
fn twiml_ok() -> axum::response::Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/xml")],
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Response></Response>",
    )
        .into_response()
}

/// Strip Twilio's `whatsapp:` channel prefix, leaving a bare E.164 number.
fn strip_whatsapp_prefix(addr: &str) -> String {
    let a = addr.trim();
    a.strip_prefix("whatsapp:").unwrap_or(a).to_string()
}

/// The configured allowlist of bare E.164 numbers, or `None` when unset (which
/// means "allow all"; the daemon's EVENTD_ADMIN_USER_IDS still gates triggering).
fn allowed_numbers() -> Option<Vec<String>> {
    let raw = std::env::var("TWILIO_ALLOWED_NUMBERS").ok()?;
    let nums: Vec<String> = raw
        .split(',')
        .map(strip_whatsapp_prefix)
        .filter(|s| !s.is_empty())
        .collect();
    if nums.is_empty() { None } else { Some(nums) }
}

/// Twilio request validation: base string = the full URL followed by each POST
/// param's key and value concatenated in alphabetical key order, then
/// HMAC-SHA1 with the auth token, base64-encoded.
fn verify_twilio_signature(
    token: &str,
    url: &str,
    params: &BTreeMap<String, String>,
    signature: &str,
) -> bool {
    let mut base = String::from(url);
    for (k, v) in params {
        base.push_str(k);
        base.push_str(v);
    }

    let mut mac =
        Hmac::<Sha1>::new_from_slice(token.as_bytes()).expect("HMAC can take key of any size");
    mac.update(base.as_bytes());
    let expected = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

    expected == signature
}

fn verify_slack_signature(secret: &str, timestamp: &str, body: &[u8], signature: &str) -> bool {
    let sig_hex = match signature.strip_prefix("v0=") {
        Some(hex) => hex,
        None => return false,
    };

    let sig_bytes = match hex::decode(sig_hex) {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };

    let basestring = format!("v0:{timestamp}:{}", String::from_utf8_lossy(body));
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(basestring.as_bytes());

    mac.verify_slice(&sig_bytes).is_ok()
}

fn verify_github_signature(secret: &str, body: &[u8], signature: &str) -> bool {
    let sig_hex = match signature.strip_prefix("sha256=") {
        Some(hex) => hex,
        None => return false,
    };

    let sig_bytes = match hex::decode(sig_hex) {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };

    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(body);

    mac.verify_slice(&sig_bytes).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twilio_signature_matches_documented_vector() {
        // Official Twilio test vector (see their request-validation docs).
        let token = "12345";
        let url = "https://mycompany.com/myapp.php?foo=1&bar=2";
        let mut params = BTreeMap::new();
        params.insert("Caller".to_string(), "+14158675309".to_string());
        params.insert("Digits".to_string(), "1234".to_string());
        params.insert("From".to_string(), "+14158675309".to_string());
        params.insert("To".to_string(), "+18005551212".to_string());

        // Expected value computed independently (HMAC-SHA1 of url + sorted
        // key+value pairs, base64-encoded, key = auth token).
        assert!(verify_twilio_signature(token, url, &params, "V4AdhXOYoGGDl714zmEWoHCrr0A="));
        assert!(!verify_twilio_signature(token, url, &params, "wrongsignature"));
    }

    #[test]
    fn strips_whatsapp_prefix() {
        assert_eq!(strip_whatsapp_prefix("whatsapp:+15551234567"), "+15551234567");
        assert_eq!(strip_whatsapp_prefix("+15551234567"), "+15551234567");
    }
}
