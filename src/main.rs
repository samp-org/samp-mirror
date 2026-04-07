mod api;
mod db;
mod indexer;
mod parse;

use clap::{Parser, Subcommand};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Parser)]
#[command(
    name = "samp-mirror",
    about = "SAMP protocol mirror -- indexes remarks and serves them via HTTP"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(long, help = "Substrate WebSocket RPC endpoint")]
    node: Option<String>,

    #[arg(long, default_value = "mirror.db", help = "SQLite database path")]
    db: String,

    #[arg(long, default_value = "8080", help = "HTTP API port")]
    port: u16,

    #[arg(
        long,
        default_value = "0",
        help = "Block to start indexing from (first run only)"
    )]
    start_block: u64,
}

#[derive(Subcommand)]
enum Command {
    /// Export a snapshot of the database
    Snapshot {
        #[arg(long, default_value = "mirror.db", help = "Source database path")]
        db: Option<String>,
        #[arg(long, default_value = "snapshot.tar.gz")]
        output: String,
    },
}

#[tokio::main]
async fn main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    if let Some(Command::Snapshot { db, output }) = &cli.command {
        let db_path = db.as_deref().unwrap_or(&cli.db);
        snapshot(db_path, output);
        return;
    }

    let node_url = cli.node.unwrap_or_else(|| {
        tracing::error!("--node is required");
        std::process::exit(1);
    });

    let (chain, ss58_prefix) = match indexer::fetch_chain_info(&node_url).await {
        Ok(info) => info,
        Err(e) => {
            tracing::error!("Failed to fetch chain info from {node_url}: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!("Chain: {chain} (SS58 prefix: {ss58_prefix})");

    let db = Arc::new(Mutex::new(db::Db::open(&cli.db)));

    {
        let d = db.lock().await;
        if d.last_block() == 0 && cli.start_block > 0 {
            d.set_last_block(cli.start_block);
            tracing::info!("Starting from block {}", cli.start_block);
        }
    }
    tracing::info!(
        "Database: {} (synced to block {})",
        cli.db,
        db.lock().await.last_block()
    );

    let state = api::AppState {
        db: db.clone(),
        chain: chain.clone(),
        ss58_prefix,
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    let app = api::router(state);
    let addr = format!("0.0.0.0:{}", cli.port);
    tracing::info!("API listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");

    tokio::select! {
        _ = indexer::run(node_url, db, ss58_prefix) => {
            tracing::error!("Indexer exited");
        }
        result = axum::serve(listener, app) => {
            if let Err(e) = result {
                tracing::error!("API server error: {e}");
            }
        }
    }
}

fn snapshot(db_path: &str, output: &str) {
    use std::fs;

    if !std::path::Path::new(db_path).exists() {
        eprintln!("Database not found: {db_path}");
        std::process::exit(1);
    }

    // Use SQLite backup API for a consistent copy
    let src = rusqlite::Connection::open(db_path).expect("open source db");
    let tmp = format!("{db_path}.snapshot");
    {
        let mut dst = rusqlite::Connection::open(&tmp).expect("open dest db");
        let backup = rusqlite::backup::Backup::new(&src, &mut dst).expect("init backup");
        backup
            .run_to_completion(100, std::time::Duration::from_millis(10), None)
            .expect("backup");
    }
    drop(src);

    let db_bytes = fs::read(&tmp).expect("read snapshot");
    let file = fs::File::create(output).expect("create output");
    let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(enc);
    let mut header = tar::Header::new_gnu();
    header.set_size(db_bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, "mirror.db", &db_bytes[..])
        .expect("write tar");
    tar.finish().expect("finish tar");

    fs::remove_file(&tmp).ok();
    let size = fs::metadata(output).map(|m| m.len()).unwrap_or(0);
    println!("Snapshot: {output} ({} bytes)", size);
}
