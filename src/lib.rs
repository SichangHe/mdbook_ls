use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::Result;
use mdbook_incremental_preview::previewing::*;
use serde_json::Value;
use tokio::{
    io::{stdin, stdout},
    sync::mpsc,
};
use tokio_gen_server::prelude::*;
use tower_lsp::{LspService, Server};
use tracing::*;

pub mod lsp;

use lsp::*;

pub async fn run_mdbook_ls() -> Result<()> {
    let (stdin, stdout) = (stdin(), stdout());
    let live_patcher = Previewer::try_new()?;
    let (service, socket) = LspService::new(|client| MDBookLS::new(client, live_patcher));
    info!(?socket, "Starting mdBook-LS");
    Server::new(stdin, stdout, socket).serve(service).await;
    Ok(())
}
