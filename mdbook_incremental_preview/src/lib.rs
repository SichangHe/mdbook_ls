use std::{
    borrow::Cow,
    cell::RefCell,
    collections::{HashMap, HashSet},
    ffi::OsStr,
    io, iter, mem,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{bail, Context};
use drop_this::*;
use futures_util::sink::SinkExt;
use handlebars::Handlebars;
use ignore::gitignore::Gitignore;
use mdbook::{
    book::{preprocessor_should_run, Book, Chapter},
    config::HtmlConfig,
    errors::*,
    preprocess::{Preprocessor, PreprocessorContext},
    renderer::{
        html_handlebars::{
            hbs_renderer::{make_data, RenderItemContext},
            search,
        },
        HtmlHandlebars, RenderContext,
    },
    theme::{self, playground_editor, Theme},
    utils, BookItem, Config, MDBook, Renderer, MDBOOK_VERSION,
};
use notify::{RecommendedWatcher, RecursiveMode::*};
use notify_debouncer_mini::{DebounceEventHandler, DebouncedEvent, Debouncer};
use serde_json::json;
use tempfile::{tempdir, TempDir};
use tokio::{
    fs::{self, File},
    io::AsyncReadExt,
    select, spawn,
    sync::{mpsc, oneshot, watch},
    task::{block_in_place, spawn_blocking, yield_now, JoinHandle},
    time::timeout,
};
use tokio_gen_server::prelude::*;
use tokio_two_join_set::TwoJoinSet;
use tracing::*;
use warp::{
    filters::{
        path::{FullPath, Tail},
        ws::{WebSocket, Ws},
        BoxedFilter,
    },
    reply::{with_header, WithHeader},
    ws::Message,
    Filter,
};

pub mod build_book;
pub mod git_ignore;
pub mod patch_registry;
pub mod previewing;
pub mod rebuilding;
pub mod rendering;
pub mod watch_files;
pub mod web_server;

use build_book::*;
use git_ignore::*;
use patch_registry::*;
use previewing::*;
use rebuilding::*;
use rendering::*;
use watch_files::*;
use web_server::*;

// NOTE: Below is adapted from
// <https://github.com/rust-lang/mdBook/blob/3bdcc0a5a6f3c85dd751350774261dbc357b02bd/src/cmd/serve.rs>.

/// The HTTP endpoint for the WebSocket used to trigger reloads when a file changes.
const LIVE_PATCH_WEBSOCKET_PATH: &str = "__mdbook_incremental_preview_live_patch";

// Serve the book at absolute path `book_root` at the given `socket_address`,
// and patch it live continuously.
pub async fn preview_continuously(
    book_root: PathBuf,
    socket_address: SocketAddr,
    open_browser: bool,
) -> Result<()> {
    let previewer = Previewer::try_new()?;
    let (handle, actor_ref) = previewer.spawn();
    actor_ref.cast(PreviewInfo::BookRoot(book_root)).await?;
    let msg = PreviewInfo::OpenPreview {
        socket_address: Some(socket_address),
        open_browser_at: open_browser.then_some("".into()),
    };
    actor_ref.cast(msg).await?;
    try_join_actor_handle(handle).await?;
    Ok(())
}

/// Runs the provided blocking function on the current thread without
/// blocking the executor,
/// then yield the control back to the executor.
pub async fn block_n_yield<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let result = block_in_place(f);
    yield_now().await;
    result
}

async fn shut_down_actor_n_log_err<A: Actor>(
    handle: ActorHandle<ActorMsg<A>>,
    actor_ref: ActorRef<A>,
    err_msg: &'static str,
) {
    actor_ref.cancel();
    if let Err(err) = try_join_actor_handle(handle).await {
        error!(?err, err_msg);
    }
}

async fn try_join_actor_handle<T>(handle: ActorHandle<T>) -> Result<()> {
    handle.await?.1
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
