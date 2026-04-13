pub mod api;
pub mod db;
pub mod indexer;

use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn serve(
    node_url: String,
    db_path: &str,
    port: u16,
    start_block: u64,
) -> Result<(), String> {
    let (chain, ss58_prefix) = indexer::fetch_chain_info(&node_url).await?;
    tracing::info!("Chain: {chain} (SS58 prefix: {ss58_prefix})");

    let db = Arc::new(Mutex::new(db::Db::open(db_path)));

    let last_indexed = db.lock().await.last_block();
    if last_indexed == 0 && start_block > 0 {
        tracing::info!(
            "Database: {db_path} (empty; first-run start_block = {start_block})"
        );
    } else {
        tracing::info!(
            "Database: {db_path} (last indexed block = {last_indexed})"
        );
    }

    let state = api::AppState {
        db: db.clone(),
        chain: chain.clone(),
        ss58_prefix,
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    let app = api::router(state);
    let addr = format!("0.0.0.0:{port}");
    tracing::info!("API listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("bind: {e}"))?;

    tokio::select! {
        _ = indexer::run(node_url, db, ss58_prefix, start_block) => {
            Err("Indexer exited".into())
        }
        result = axum::serve(listener, app) => {
            result.map_err(|e| format!("API server error: {e}"))
        }
    }
}
