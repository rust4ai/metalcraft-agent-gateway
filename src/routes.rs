use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{delete, get, patch, post, put},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{AppState, auth, platform::PlatformError};

type S = Arc<AppState>;

pub fn router() -> Router<S> {
    Router::new()
        .route("/messages", post(send_message))
        .route("/messages/{message_id}", patch(edit_message))
        .route(
            "/messages/{message_id}/reactions",
            put(add_reaction),
        )
        .route(
            "/channels/{channel_id}/messages",
            get(get_messages),
        )
        .route("/channels/{channel_id}", get(get_channel_info))
        .route("/subscribers", post(create_subscriber).get(list_subscribers))
        .route("/subscribers/{id}", delete(delete_subscriber))
        .layer(middleware::from_fn(auth::require_api_key))
}

fn platform_err(e: PlatformError) -> impl IntoResponse {
    (
        StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_GATEWAY),
        Json(json!({ "error": e.message })),
    )
}

// ── POST /messages ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SendMessageBody {
    channel_id: String,
    content: String,
    message_reference_id: Option<String>,
    platform: String,
}

async fn send_message(
    State(state): State<S>,
    Json(body): Json<SendMessageBody>,
) -> Result<Json<Value>, impl IntoResponse> {
    let p = state.resolve(&body.platform).map_err(platform_err)?;
    p.send_message(
        &body.channel_id,
        &body.content,
        body.message_reference_id.as_deref(),
    )
    .await
    .map(Json)
    .map_err(platform_err)
}

// ── PATCH /messages/:message_id ─────────────────────────────────────────

#[derive(Deserialize)]
struct EditMessageBody {
    channel_id: String,
    content: String,
    platform: String,
}

async fn edit_message(
    State(state): State<S>,
    Path(message_id): Path<String>,
    Json(body): Json<EditMessageBody>,
) -> Result<Json<Value>, impl IntoResponse> {
    let p = state.resolve(&body.platform).map_err(platform_err)?;
    p.edit_message(&body.channel_id, &message_id, &body.content)
        .await
        .map(Json)
        .map_err(platform_err)
}

// ── PUT /messages/:message_id/reactions ──────────────────────────────────

#[derive(Deserialize)]
struct AddReactionBody {
    channel_id: String,
    emoji: String,
    platform: String,
}

async fn add_reaction(
    State(state): State<S>,
    Path(message_id): Path<String>,
    Json(body): Json<AddReactionBody>,
) -> Result<Json<Value>, impl IntoResponse> {
    let p = state.resolve(&body.platform).map_err(platform_err)?;
    p.add_reaction(&body.channel_id, &message_id, &body.emoji)
        .await
        .map(Json)
        .map_err(platform_err)
}

// ── GET /channels/:channel_id/messages?limit=N&platform=X ───────────────

#[derive(Deserialize)]
struct GetMessagesQuery {
    limit: Option<u64>,
    platform: String,
}

async fn get_messages(
    State(state): State<S>,
    Path(channel_id): Path<String>,
    Query(q): Query<GetMessagesQuery>,
) -> Result<Json<Value>, impl IntoResponse> {
    let p = state.resolve(&q.platform).map_err(platform_err)?;
    let limit = q.limit.unwrap_or(10).min(50);
    p.get_messages(&channel_id, limit)
        .await
        .map(Json)
        .map_err(platform_err)
}

// ── GET /channels/:channel_id?platform=X ────────────────────────────────

#[derive(Deserialize)]
struct ChannelInfoQuery {
    platform: String,
}

async fn get_channel_info(
    State(state): State<S>,
    Path(channel_id): Path<String>,
    Query(q): Query<ChannelInfoQuery>,
) -> Result<Json<Value>, impl IntoResponse> {
    let p = state.resolve(&q.platform).map_err(platform_err)?;
    p.get_channel_info(&channel_id)
        .await
        .map(Json)
        .map_err(platform_err)
}

// ── Subscriber CRUD ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CreateSubscriberBody {
    url: String,
    events: Vec<String>,
    platforms: Option<Vec<String>>,
    secret: Option<String>,
}

/// Redact the secret field from a subscriber for API responses.
fn redact_secret(mut v: Value) -> Value {
    if let Some(obj) = v.as_object_mut() {
        if obj.contains_key("secret") {
            obj.remove("secret");
        }
    }
    v
}

async fn create_subscriber(
    State(state): State<S>,
    Json(body): Json<CreateSubscriberBody>,
) -> impl IntoResponse {
    let sub = state
        .subscriber_store
        .add(body.url, body.events, body.platforms, body.secret)
        .await;
    (StatusCode::CREATED, Json(redact_secret(serde_json::to_value(sub).unwrap())))
}

async fn list_subscribers(State(state): State<S>) -> Json<Value> {
    let subs = state.subscriber_store.list().await;
    let redacted: Vec<Value> = subs
        .into_iter()
        .map(|s| redact_secret(serde_json::to_value(s).unwrap()))
        .collect();
    Json(serde_json::to_value(redacted).unwrap())
}

async fn delete_subscriber(
    State(state): State<S>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if state.subscriber_store.remove(&id).await {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}
