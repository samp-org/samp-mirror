use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;

use crate::db::Db;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Db>>,
    pub chain: String,
    pub ss58_prefix: u16,
    pub version: String,
}

#[derive(serde::Deserialize)]
pub struct AfterParam {
    after: Option<u64>,
}

#[derive(serde::Deserialize)]
pub struct RemarksQuery {
    r#type: Option<String>,
    sender: Option<String>,
    after: Option<u64>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/channels", get(channels))
        .route(
            "/v1/channels/{block}/{index}/messages",
            get(channel_messages),
        )
        .route("/v1/remarks", get(remarks))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let synced_to = state.db.lock().await.last_block();
    Json(serde_json::json!({
        "chain": state.chain,
        "ss58_prefix": state.ss58_prefix,
        "synced_to": synced_to,
        "version": state.version,
    }))
}

async fn channels(State(state): State<AppState>) -> Json<serde_json::Value> {
    let rows = state.db.lock().await.channels();
    Json(serde_json::json!(rows))
}

async fn channel_messages(
    State(state): State<AppState>,
    Path((block, index)): Path<(u32, u16)>,
    Query(params): Query<AfterParam>,
) -> Json<serde_json::Value> {
    let after = params.after.unwrap_or(0);
    let rows = state.db.lock().await.channel_messages(block, index, after);
    Json(serde_json::json!(rows))
}

async fn remarks(
    State(state): State<AppState>,
    Query(params): Query<RemarksQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let after = params.after.unwrap_or(0);

    if let Some(type_str) = &params.r#type {
        let ct = parse_content_type(type_str).ok_or(StatusCode::BAD_REQUEST)?;
        let rows = state.db.lock().await.remarks_by_type(ct, after);
        return Ok(Json(serde_json::json!(rows)));
    }

    if let Some(sender) = &params.sender {
        let rows = state.db.lock().await.remarks_by_sender(sender, after);
        return Ok(Json(serde_json::json!(rows)));
    }

    Err(StatusCode::BAD_REQUEST)
}

pub fn parse_content_type(s: &str) -> Option<u8> {
    if let Some(hex) = s.strip_prefix("0x") {
        u8::from_str_radix(hex, 16).ok()
    } else {
        s.parse().ok()
    }
}
