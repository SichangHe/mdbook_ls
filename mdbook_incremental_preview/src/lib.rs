use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    ffi::OsStr,
    io, iter, mem,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{bail, Context};
use drop_this::*;
use futures_util::sink::SinkExt;
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
    theme::{self, playground_editor, Theme},
    utils, BookItem, MDBook,
};
use notify::{RecommendedWatcher, RecursiveMode::*};
use notify_debouncer_mini::{DebounceEventHandler, DebouncedEvent, Debouncer};
use serde_json::json;
use tempfile::tempdir;
use tokio::{
    fs::{self, File},
    io::AsyncReadExt,
    select,
    sync::{mpsc, oneshot, watch},
    task::{block_in_place, yield_now, JoinSet},
    time::timeout,
};
use tokio_gen_server::{actor::ActorRunExt, prelude::*};
use tokio_util::sync::CancellationToken;
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
pub mod rebuilding;
pub mod rendering;
pub mod watch_files;
pub mod web_server;

use {
    build_book::*, git_ignore::*, patch_registry::*, rebuilding::*, rendering::*, watch_files::*,
    web_server::*,
};

// NOTE: Below is adapted from
// <https://github.com/rust-lang/mdBook/blob/3bdcc0a5a6f3c85dd751350774261dbc357b02bd/src/cmd/serve.rs>.

/// The HTTP endpoint for the WebSocket used to trigger reloads when a file changes.
const LIVE_PATCH_WEBSOCKET_PATH: &str = "__mdbook_incremental_preview_live_patch";

// Serve the book at absolute path `book_root` at the given `socket_address`,
// and patch it live continuously.
pub async fn live_patch_continuously(
    book_root: PathBuf,
    socket_address: SocketAddr,
    open_browser: bool,
) -> Result<()> {
    let build_temp_dir = tempdir()?; // Do not drop; preserve the temporary directory.
    let build_dir = build_temp_dir.path();
    yield_now().await;

    let serving_url = format!("http://{}", socket_address);
    info!(?serving_url, ?book_root, ?build_dir, "Will serve");

    let mut join_set = JoinSet::new();
    let cancel_token = CancellationToken::new();
    let patch_registry_ref = spawn_actor_n_log_err(
        PatchRegistry::default(),
        &mut join_set,
        8,
        &cancel_token,
        "PatchRegistry exit",
    );

    let (info_tx, info_rx) = mpsc::channel(8);
    let rebuilder = Rebuilder::new(
        book_root.clone(),
        build_dir.to_owned(),
        info_tx.clone(),
        patch_registry_ref.clone(),
        open_browser.then_some(serving_url),
    );
    let rebuilder_ref = spawn_actor_n_log_err(
        rebuilder,
        &mut join_set,
        64,
        &cancel_token,
        "Rebuilder exit",
    );

    join_set.spawn(serve_reloading(
        book_root,
        socket_address,
        build_dir.to_owned(),
        rebuilder_ref,
        info_rx,
        patch_registry_ref,
    ));

    while join_set.join_next().await.is_some() {}
    Ok(())
}

fn spawn_actor_n_log_err<A>(
    actor: A,
    join_set: &mut JoinSet<()>,
    channel_capacity: usize,
    cancel_token: &CancellationToken,
    exit_msg: &'static str,
) -> ActorRef<A>
where
    A: Actor + Send + 'static,
    ActorMsg<A>: Send,
{
    let (msg_sender, msg_receiver) = mpsc::channel(channel_capacity);
    let a_ref = ActorRef::<A> {
        msg_sender,
        cancellation_token: cancel_token.child_token(),
    };
    let env = a_ref.clone();
    join_set.spawn(async move {
        if let (_, Err(err)) = actor.run_and_handle_exit(env, msg_receiver).await {
            error!(?err, exit_msg);
        }
    });
    a_ref
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
