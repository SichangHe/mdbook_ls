use std::path::{PathBuf, Path};

use anyhow::Result;
use drop_this::*;
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

pub mod live_patching;
pub mod lsp;

use {live_patching::*, lsp::*};

pub async fn run_mdbook_ls() {
    let (stdin, stdout) = (stdin(), stdout());
    let (service, socket) = LspService::new(MDBookLS::new);
    info!(?socket, "Starting mdBook-LS");
    Server::new(stdin, stdout, socket).serve(service).await;
}
