use tokio::select;

use super::*;

pub async fn serve_reloading(
    address: SocketAddr,
    build_dir: PathBuf,
    reload_tx: broadcast::Sender<Message>,
    mut info_rx: tokio::sync::mpsc::Receiver<ServeInfo>,
) {
    let Some(mut info) = info_rx.recv().await else {
        error!("Did not start server because all info senders have been dropped.");
        return;
    };
    info!("Starting server with reloading.");
    loop {
        let ServeInfo { src_dir, file_404 } = info.clone();
        let maybe_maybe_info = select! {
            _ = serve(src_dir, build_dir.clone(), address, reload_tx.clone(), file_404) => None,
            maybe_info = info_rx.recv() => Some(maybe_info),
        };
        match maybe_maybe_info {
            None => {}
            Some(None) => {
                info!("Stopping server reloading because all info senders have been dropped.");
                return;
            }
            Some(Some(new_info)) => info = new_info,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ServeInfo {
    pub src_dir: PathBuf,
    pub file_404: PathBuf,
}

pub async fn serve(
    src_dir: PathBuf,
    build_dir: PathBuf,
    address: SocketAddr,
    reload_tx: broadcast::Sender<Message>,
    file_404: PathBuf,
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
    let book_route = warp::fs::dir(build_dir.clone()).or(warp::fs::dir(src_dir));
    // The fallback route for 404 errors
    let fallback_route = warp::fs::file(file_404)
        .map(|reply| warp::reply::with_status(reply, warp::http::StatusCode::NOT_FOUND));
    let routes = livereload.or(book_route).or(fallback_route);

    std::panic::set_hook(Box::new(move |panic_info| {
        // exit if serve panics
        error!("Unable to serve: {}", panic_info);
        std::process::exit(1);
    }));

    warp::serve(routes).run(address).await;
}
