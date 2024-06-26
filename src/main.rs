use std::io::stderr;

use anyhow::Result;
use clap::Parser;
use mdbook_ls::run_mdbook_ls;
use tracing::*;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(stderr)
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let app = App::parse();
    debug!(?app);
    run_mdbook_ls().await
}

#[derive(Clone, Debug, Parser)]
#[command(
    version,
    about,
    long_about = r#"
TODO.
"#
)]
struct App {}
