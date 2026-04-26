//! wxbtrv-web — local-only HTTP server + embedded React UI for managing
//! wxbtrv.db, the same surface area db_config exposes via CLI.
//!
//! Run it: `wxbtrv-web` opens a random localhost port, prints the URL,
//! launches your default browser, and runs until you Ctrl-C it. The UI
//! is embedded in the binary; no external assets, no daemon.

use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod api;
mod embed;
mod state;

use state::AppState;

#[derive(Parser, Debug)]
#[command(
    name = "wxbtrv-web",
    about = "Local web UI for managing wxbtrv.db",
    version
)]
struct Cli {
    /// Path to wxbtrv.db. Defaults match the runtime: CWD, then
    /// ../bin/wxbtrv.db / C:\WatkinsX\bin\wxbtrv.db, or override here.
    #[arg(long)]
    db: Option<PathBuf>,

    /// Bind host. Default 127.0.0.1 (localhost only).
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Bind port. 0 = OS-assigned (default).
    #[arg(long, default_value_t = 0)]
    port: u16,

    /// Don't open a browser on startup.
    #[arg(long)]
    no_browser: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer())
        .init();

    let cli = Cli::parse();

    let state = AppState::new(cli.db.clone());
    let app = api::router(state);

    let addr: SocketAddr = format!("{}:{}", cli.host, cli.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    let url = format!("http://{}/", local);

    println!("wxbtrv-web listening on {url}");
    println!("  db: {}", state_db_display(&cli.db));
    println!("  press Ctrl-C to stop");

    if !cli.no_browser {
        if let Err(e) = opener::open_browser(&url) {
            eprintln!("could not open browser ({e}) — open {url} manually");
        }
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn state_db_display(p: &Option<PathBuf>) -> String {
    p.as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(auto-discover)".to_string())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    println!("\nshutting down");
}
