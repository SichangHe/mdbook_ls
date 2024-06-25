use std::path::{Path, PathBuf};

use anyhow::Result;
use drop_this::*;
use mdbook_incremental_preview::live_patching::*;
use serde_json::Value;
use tokio::{
    io::{stdin, stdout},
    spawn,
    sync::mpsc,
    task::JoinSet,
};
use tokio_gen_server::prelude::*;
use tower_lsp::{LspService, Server};
use tracing::*;

pub mod lsp;

use lsp::*;

pub async fn run_mdbook_ls() -> Result<()> {
    let (stdin, stdout) = (stdin(), stdout());
    let live_patcher = LivePatcher::try_new()?;
    let (service, socket) = LspService::new(|client| MDBookLS::new(client, live_patcher));
    info!(?socket, "Starting mdBook-LS");
    Server::new(stdin, stdout, socket).serve(service).await;
    Ok(())
}
