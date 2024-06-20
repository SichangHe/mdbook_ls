use std::{
    collections::{HashMap, HashSet},
    env,
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read},
    mem,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::mpsc::{channel, Receiver},
    thread::sleep,
    time::Duration,
};

use anyhow::{bail, Context};
use futures_util::{sink::SinkExt, StreamExt};
use handlebars::Handlebars;
use ignore::gitignore::Gitignore;
use mdbook::{
    book::{Book, Chapter},
    config::HtmlConfig,
    errors::*,
    renderer::{
        html_handlebars::{
            hbs_renderer::{make_data, RenderItemContext},
            search,
        },
        HtmlHandlebars, RenderContext,
    },
    theme::Theme,
    utils::{self, fs::get_404_output_file},
    BookItem, MDBook,
};
use notify::{RecommendedWatcher, RecursiveMode::*};
use notify_debouncer_mini::{DebounceEventHandler, DebouncedEvent, Debouncer};
use serde_json::json;
use tempfile::tempdir;
use tokio::{
    sync::broadcast,
    task::{block_in_place, yield_now, JoinSet},
};
use tracing::*;
use warp::{ws::Message, Filter};

pub mod build_book;
pub mod git_ignore;
pub mod rebuilding;
pub mod rendering;
pub mod watch_files;
pub mod web_server;

use {build_book::*, git_ignore::*, rebuilding::*, rendering::*, watch_files::*, web_server::*};

// NOTE: Below is adapted from
// <https://github.com/rust-lang/mdBook/blob/3bdcc0a5a6f3c85dd751350774261dbc357b02bd/src/cmd/serve.rs>.

/// The HTTP endpoint for the websocket used to trigger reloads when a file changes.
const LIVE_RELOAD_ENDPOINT: &str = "__livereload";

// Serve command implementation
pub async fn execute(socket_address: SocketAddr, open_browser: bool) -> Result<()> {
    let build_temp_dir = tempdir()?; // Do not drop; preserve the temporary directory.
    let build_dir = build_temp_dir.path();
    yield_now().await;

    let serving_url = format!("http://{}", socket_address);
    info!(?serving_url, ?build_dir, "Will serve");

    let mut join_set = JoinSet::new();
    let (tx, info_tx) = {
        // A channel used to broadcast to any websockets to reload when a file changes.
        let (tx, _rx) = broadcast::channel::<Message>(100);
        let reload_tx = tx.clone();

        // TODO: A watch channel may be better.
        let (info_tx, info_rx) = tokio::sync::mpsc::channel(8);
        let build_dir = build_dir.to_path_buf();
        join_set.spawn(serve_reloading(
            socket_address,
            build_dir,
            reload_tx,
            info_rx,
        ));
        (tx, info_tx)
    };

    rebuild_on_change(
        env::current_dir()?,
        serving_url,
        build_dir,
        info_tx,
        open_browser,
        &move || {
            let _ = tx.send(Message::text("reload"));
        },
    )
    .await?;

    join_set.shutdown().await;
    Ok(())
}

fn open<P: AsRef<OsStr>>(path: P) {
    match opener::open(path) {
        Err(err) => {
            error!(?err, "opening web browser.")
        }
        Ok(_) => {
            info!("Opened web browser.")
        }
    }
}
