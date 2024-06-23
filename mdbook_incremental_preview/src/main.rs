use clap::Parser;
use std::{io::stderr, net::IpAddr, path::PathBuf};

use anyhow::Result;
use mdbook_incremental_preview::live_patch_continuously;
use tracing::*;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_ansi(true)
        .with_writer(stderr)
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    debug!("Starting");

    let args = Args::parse();
    let book_root = args.dir.canonicalize()?;
    let socket_address = (args.hostname, args.port).into();
    live_patch_continuously(book_root, socket_address, args.open).await
}

#[derive(Parser)]
#[command(
    about = "Serves an mdBook project and live patch it on changes.",
    version
)]
struct Args {
    /// Root directory for the book (Defaults to the current directory when omitted)
    #[arg(default_value = ".")]
    dir: PathBuf,

    /// Hostname to listen on for HTTP connections
    #[arg(short = 'n', long, default_value = "127.0.0.1")]
    hostname: IpAddr,

    /// Port to use for HTTP connections
    #[arg(short, long, default_value_t = 3000)]
    port: u16,

    /// Opens the compiled book in a web browser
    #[arg(short, long, default_value_t = true)]
    open: bool,
}
