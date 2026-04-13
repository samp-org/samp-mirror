use clap::{Parser, Subcommand};

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
        match samp_mirror::db::snapshot(db_path, output) {
            Ok(size) => println!("Snapshot: {output} ({size} bytes)"),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }

    let node_url = cli.node.unwrap_or_else(|| {
        tracing::error!("--node is required");
        std::process::exit(1);
    });

    if let Err(e) = samp_mirror::serve(node_url, &cli.db, cli.port, cli.start_block).await {
        tracing::error!("{e}");
        std::process::exit(1);
    }
}
