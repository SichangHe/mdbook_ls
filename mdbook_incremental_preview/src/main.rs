use std::io::stderr;

use anyhow::*;
use mdbook_incremental_preview::execute;
use tracing::*;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_ansi(true)
        .with_writer(stderr)
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    debug!("Starting");
    execute()
}
