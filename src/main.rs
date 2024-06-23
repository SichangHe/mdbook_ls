use std::io::stderr;

use clap::Parser;
use mdbook_ls::run;
use tracing::*;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_ansi(true)
        .with_writer(stderr)
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let app = App::parse();
    debug!(?app);
    run().await;
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
