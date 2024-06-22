use std::{
    collections::{HashMap, HashSet},
    env,
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read},
    iter, mem,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{anyhow, bail, Context};
use futures_util::{sink::SinkExt, FutureExt, StreamExt};
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
    utils::{self, fs::get_404_output_file},
    BookItem, MDBook,
};
use notify::{RecommendedWatcher, RecursiveMode::*};
use notify_debouncer_mini::{DebounceEventHandler, DebouncedEvent, Debouncer};
use serde_json::json;
use tempfile::tempdir;
use tokio::{
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
        path::FullPath,
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

/// The HTTP endpoint for the websocket used to trigger reloads when a file changes.
const LIVE_PATCH_WEBSOCKET_PATH: &str = "__mdbook_incremental_preview_live_patch";

// Serve command implementation
pub async fn execute(socket_address: SocketAddr, open_browser: bool) -> Result<()> {
    let build_temp_dir = tempdir()?; // Do not drop; preserve the temporary directory.
    let build_dir = build_temp_dir.path();
    yield_now().await;

    let book_root = env::current_dir()?;
    let serving_url = format!("http://{}", socket_address);
    info!(?serving_url, ?book_root, ?build_dir, "Will serve");

    let mut join_set = JoinSet::new();
    let patch_registry_ref = {
        let registry = PatchRegistry::default();
        let (msg_sender, msg_receiver) = tokio::sync::mpsc::channel(8);
        let a_ref = ActorRef::<PatchRegistry> {
            msg_sender,
            cancellation_token: CancellationToken::new(),
        };
        let env = a_ref.clone();
        join_set.spawn(
            registry
                .run_and_handle_exit(env, msg_receiver)
                .then(|(_, r)| async {
                    if let Err(err) = r {
                        error!(?err, "PatchRegistry exit");
                    }
                }),
        );
        a_ref
    };

    let (file_event_tx, file_event_rx) = mpsc::channel(64);
    let info_tx = {
        // TODO: A watch channel may be better.
        let (info_tx, info_rx) = mpsc::channel(8);
        let book_root = book_root.clone();
        let build_dir = build_dir.to_path_buf();
        let file_event_tx = file_event_tx.clone();
        let patch_registry_ref = patch_registry_ref.clone();
        join_set.spawn(serve_reloading(
            book_root,
            socket_address,
            build_dir,
            file_event_tx,
            info_rx,
            patch_registry_ref,
        ));
        info_tx
    };

    rebuild_on_change(
        book_root,
        serving_url,
        build_dir,
        info_tx,
        file_event_tx,
        file_event_rx,
        open_browser,
        patch_registry_ref,
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

pub trait DropResult {
    /// Drop this `Result` as an alternative to calling `_ =` which
    /// may accidentally ignore e.g. a `Future`.
    /// This is especially useful when sending a message through a channel.
    fn drop_result(self);
}

impl<T, E> DropResult for Result<T, E> {
    fn drop_result(self) {}
}
