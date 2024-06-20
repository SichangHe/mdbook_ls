use std::io::stderr;

use anyhow::*;
use mdbook_incremental_preview::execute;
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

    // TODO: Currently hardcoded.
    let socket_address = "127.0.0.1:3000".parse()?;
    let open_browser = true;

    execute(socket_address, open_browser).await
}
