use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::mpsc::{channel, Receiver},
    thread::sleep,
    time::Duration,
};

use anyhow::Context;
use futures_util::sink::SinkExt;
use futures_util::StreamExt;
use mdbook::errors::*;
use mdbook::utils::fs::get_404_output_file;
use mdbook::MDBook;
use std::net::{SocketAddr, ToSocketAddrs};
use tokio::sync::broadcast;
use warp::ws::Message;
use warp::Filter;

use ignore::gitignore::Gitignore;
use notify::{RecommendedWatcher, RecursiveMode::*};
use notify_debouncer_mini::{DebounceEventHandler, DebouncedEvent, Debouncer};
use tracing::*;

pub mod build_book;
pub mod git_ignore;
pub mod rebuilding;
pub mod watch_files;
pub mod web_server;

use {build_book::*, git_ignore::*, rebuilding::*, watch_files::*, web_server::*};

// NOTE: Below is adapted from
// <https://github.com/rust-lang/mdBook/blob/3bdcc0a5a6f3c85dd751350774261dbc357b02bd/src/cmd/serve.rs>.

/// The HTTP endpoint for the websocket used to trigger reloads when a file changes.
const LIVE_RELOAD_ENDPOINT: &str = "__livereload";

// Serve command implementation
pub fn execute() -> Result<()> {
    let book_dir = env::current_dir()?;
    let mut book = MDBook::load(book_dir)?;
    config_and_build_book(&mut book)?;

    // TODO: Currently hardcoded.
    let port = "3000";
    let hostname = "localhost";
    let open_browser = true;

    let address = format!("{}:{}", hostname, port);

    let sockaddr: SocketAddr = address
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow::anyhow!("no address found for {}", address))?;
    let build_dir = book.build_dir_for("html");
    let input_404 = book
        .config
        .get("output.html.input-404")
        .and_then(toml::Value::as_str)
        .map(ToString::to_string);
    let file_404 = get_404_output_file(&input_404);

    // A channel used to broadcast to any websockets to reload when a file changes.
    let (tx, _rx) = tokio::sync::broadcast::channel::<Message>(100);

    let reload_tx = tx.clone();
    let thread_handle = std::thread::spawn(move || {
        serve(build_dir, sockaddr, reload_tx, &file_404);
    });

    let serving_url = format!("http://{}", address);
    info!("Serving on: {}", serving_url);

    if open_browser {
        open(serving_url);
    }

    rebuild_on_change(&mut book, &move || {
        let _ = tx.send(Message::text("reload"));
    });

    let _ = thread_handle.join();

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
