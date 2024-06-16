use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::mpsc::{channel, Receiver},
    thread::sleep,
    time::Duration,
};

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
use notify::{FsEventWatcher, RecursiveMode::*};
use notify_debouncer_mini::{DebounceEventHandler, DebouncedEvent, Debouncer};
use tracing::*;

// NOTE: Below is adapted from
// <https://github.com/rust-lang/mdBook/blob/3bdcc0a5a6f3c85dd751350774261dbc357b02bd/src/cmd/serve.rs>.

/// The HTTP endpoint for the websocket used to trigger reloads when a file changes.
const LIVE_RELOAD_ENDPOINT: &str = "__livereload";

// Serve command implementation
pub fn execute() -> Result<()> {
    let book_dir = env::current_dir()?;
    let mut book = MDBook::load(&book_dir)?;

    // TODO: Currently hardcoded.
    let port = "3000";
    let hostname = "localhost";
    let open_browser = true;

    let address = format!("{}:{}", hostname, port);

    let update_config = |book: &mut MDBook| {
        book.config
            .set("output.html.live-reload-endpoint", LIVE_RELOAD_ENDPOINT)
            .expect("live-reload-endpoint update failed");
        // Override site-url for local serving of the 404 file
        book.config.set("output.html.site-url", "/").unwrap();
    };
    update_config(&mut book);
    book.build()?;

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

    rebuild_on_change(&book_dir, &update_config, &move || {
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

#[tokio::main]
async fn serve(
    build_dir: PathBuf,
    address: SocketAddr,
    reload_tx: broadcast::Sender<Message>,
    file_404: &str,
) {
    // A warp Filter which captures `reload_tx` and provides an `rx` copy to
    // receive reload messages.
    let sender = warp::any().map(move || reload_tx.subscribe());

    // A warp Filter to handle the livereload endpoint. This upgrades to a
    // websocket, and then waits for any filesystem change notifications, and
    // relays them over the websocket.
    let livereload = warp::path(LIVE_RELOAD_ENDPOINT)
        .and(warp::ws())
        .and(sender)
        .map(|ws: warp::ws::Ws, mut rx: broadcast::Receiver<Message>| {
            ws.on_upgrade(move |ws| async move {
                let (mut user_ws_tx, _user_ws_rx) = ws.split();
                trace!("websocket got connection");
                if let Ok(m) = rx.recv().await {
                    trace!("notify of reload");
                    let _ = user_ws_tx.send(m).await;
                }
            })
        });
    // A warp Filter that serves from the filesystem.
    let book_route = warp::fs::dir(build_dir.clone());
    // The fallback route for 404 errors
    let fallback_route = warp::fs::file(build_dir.join(file_404))
        .map(|reply| warp::reply::with_status(reply, warp::http::StatusCode::NOT_FOUND));
    let routes = livereload.or(book_route).or(fallback_route);

    std::panic::set_hook(Box::new(move |panic_info| {
        // exit if serve panics
        error!("Unable to serve: {}", panic_info);
        std::process::exit(1);
    }));

    warp::serve(routes).run(address).await;
}

// NOTE: Below is adapted from
// <https://github.com/rust-lang/mdBook/blob/3bdcc0a5a6f3c85dd751350774261dbc357b02bd/src/cmd/watch/native.rs>.

pub fn rebuild_on_change(
    book_dir: &Path,
    update_config: &dyn Fn(&mut MDBook),
    post_build: &dyn Fn(),
) {
    let mut book = MDBook::load(book_dir).unwrap_or_else(|e| {
        error!("failed to load book: {e}");
        std::process::exit(1);
    });

    // Create a channel to receive the events.
    let (tx, rx) = channel();
    let _debouncer_to_keep_watcher_alive = watch_file_changes(&book, tx);

    let config_location = book_dir.join("book.toml");
    info!(?config_location);
    loop {
        // TODO: Instead of getting the paths,
        // preserve the events and use the info they contain.
        let paths = recv_changed_paths(&book, &rx);
        if !paths.is_empty() {
            info!(?paths, "Files changed");
            if paths.contains(&config_location) {
                // TODO: Leverage this info to avoid full rebuilds.
                // The configuration changed, perform a full rebuild.
            }
            match MDBook::load(book_dir) {
                Ok(mut b) => {
                    update_config(&mut b);
                    if let Err(err) = b.build() {
                        error!(?err, "failed to build the book");
                    } else {
                        post_build();
                    }
                    book = b;
                    info!("rebuilt the book");
                }
                Err(err) => error!(?err, "failed to load book config"),
            }
        }
    }
}

const DEBOUNCER_TIMEOUT: Duration = Duration::from_millis(20);

pub(crate) fn watch_file_changes<F>(book: &MDBook, event_handler: F) -> Debouncer<FsEventWatcher>
where
    F: DebounceEventHandler,
{
    let mut debouncer = match notify_debouncer_mini::new_debouncer(DEBOUNCER_TIMEOUT, event_handler)
    {
        Ok(d) => d,
        Err(err) => {
            error!(?err, "Trying to watch files");
            std::process::exit(1)
        }
    };

    let watcher = debouncer.watcher();

    // Add the source directory to the watcher
    if let Err(err) = watcher.watch(&book.source_dir(), Recursive) {
        error!(source_dir = ?book.source_dir(), ?err, "watching");
        std::process::exit(1);
    };

    let _ = watcher.watch(&book.theme_dir(), Recursive);

    // Add the book.toml file to the watcher if it exists
    let _ = watcher.watch(&book.root.join("book.toml"), NonRecursive);

    for dir in &book.config.build.extra_watch_dirs {
        let path = book.root.join(dir);
        let canonical_path = path.canonicalize().unwrap_or_else(|err| {
            error!(?path, ?err, "Watching extra directory");
            std::process::exit(1);
        });

        if let Err(err) = watcher.watch(&canonical_path, Recursive) {
            error!(?canonical_path, ?err, "Watching extra directory",);
            std::process::exit(1);
        }
    }

    info!("Listening for file changes.");
    debouncer
}

const EVENT_RECEIVE_TIMEOUT: Duration = Duration::from_millis(50);

pub(crate) fn recv_changed_paths(
    book: &MDBook,
    rx: &Receiver<notify::Result<Vec<DebouncedEvent>>>,
) -> Vec<PathBuf> {
    let first_event = rx.recv().unwrap();
    sleep(EVENT_RECEIVE_TIMEOUT);
    let other_events = rx.try_iter();

    let all_events = std::iter::once(first_event).chain(other_events);

    let paths: Vec<_> = all_events
        .filter_map(|event| match event {
            Ok(events) => Some(events),
            Err(err) => {
                warn!(?err, "Watching for changes");
                None
            }
        })
        .flatten()
        .map(|event| event.path)
        .collect();

    // If we are watching files outside the current repository (via extra-watch-dirs), then they are definitionally
    // ignored by gitignore. So we handle this case by including such files into the watched paths list.
    let any_external_paths = paths.iter().filter(|p| !p.starts_with(&book.root)).cloned();
    let mut paths = remove_ignored_files(&book.root, &paths[..]);
    paths.extend(any_external_paths);

    paths
}

fn remove_ignored_files(book_root: &Path, paths: &[PathBuf]) -> Vec<PathBuf> {
    if paths.is_empty() {
        return vec![];
    }

    match find_gitignore(book_root) {
        Some(gitignore_path) => {
            let (ignore, err) = Gitignore::new(&gitignore_path);
            if let Some(err) = err {
                warn!(
                    "error reading gitignore `{}`: {err}",
                    gitignore_path.display()
                );
            }
            filter_ignored_files(ignore, paths)
        }
        None => {
            // There is no .gitignore file.
            paths.iter().map(|path| path.to_path_buf()).collect()
        }
    }
}

fn find_gitignore(book_root: &Path) -> Option<PathBuf> {
    book_root
        .ancestors()
        .map(|p| p.join(".gitignore"))
        .find(|p| p.exists())
}

// Note: The usage of `canonicalize` may encounter occasional failures on the Windows platform, presenting a potential risk.
// For more details, refer to [Pull Request #2229](https://github.com/rust-lang/mdBook/pull/2229#discussion_r1408665981).
fn filter_ignored_files(ignore: Gitignore, paths: &[PathBuf]) -> Vec<PathBuf> {
    let ignore_root = ignore
        .path()
        .canonicalize()
        .expect("ignore root canonicalize error");

    paths
        .iter()
        .filter(|path| {
            let relative_path = pathdiff::diff_paths(path, &ignore_root)
                .expect("One of the paths should be an absolute");
            !ignore
                .matched_path_or_any_parents(&relative_path, relative_path.is_dir())
                .is_ignore()
        })
        .map(|path| path.to_path_buf())
        .collect()
}
