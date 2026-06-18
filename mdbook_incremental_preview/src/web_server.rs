use super::*;

pub async fn serve_reloading(
    book_root: PathBuf,
    address: SocketAddr,
    build_dir: PathBuf,
    rebuilder_ref: ActorRef<Rebuilder>,
    mut info_rx: mpsc::Receiver<ServeInfo>,
    patch_registry_ref: ActorRef<PatchRegistry>,
) {
    let Some(mut info) = info_rx.recv().await else {
        error!("Did not start server because all info senders have been dropped.");
        return;
    };
    info!("Starting server with reloading.");
    let mut info_buf = Vec::new();
    loop {
        let maybe_maybe_info = select! {
            _ = serve(book_root.clone(), build_dir.clone(), address, rebuilder_ref.clone(), info.clone(), patch_registry_ref.clone()) => None,
            maybe_info = info_rx.recv() => Some(maybe_info),
        };
        match maybe_maybe_info {
            None => {}
            Some(None) => {
                info!("Stopping server reloading because all info senders have been dropped.");
                return;
            }
            Some(Some(new_info)) => {
                // Consume all the info in the channel.
                timeout(Duration::ZERO, info_rx.recv_many(&mut info_buf, usize::MAX))
                    .await
                    .drop_result();
                info = info_buf.pop().unwrap_or(new_info);
                info_buf.clear();
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ServeInfo {
    pub src_dir: PathBuf,
    pub file_404: PathBuf,
}

pub async fn serve(
    _book_root: PathBuf,
    build_dir: PathBuf,
    address: SocketAddr,
    rebuilder_ref: ActorRef<Rebuilder>,
    info: ServeInfo,
    patch_registry_ref: ActorRef<PatchRegistry>,
) {
    let ServeInfo { src_dir, file_404 } = info;

    // Handle WebSockets for live-patching.
    let p_ref = patch_registry_ref.clone();
    let live_patch = warp::path(LIVE_PATCH_WEBSOCKET_PATH)
        .and(warp::path::tail())
        .and(warp::ws())
        .and(warp::any().map(move || p_ref.clone()))
        .map(move |tail: Tail, ws: Ws, patch_registry_ref| {
            ws.on_upgrade(move |mut ws| async move {
                if let Err(err) = handle_ws(tail.as_str(), &mut ws, patch_registry_ref).await {
                    error!(?err, "Handling WebSocket");
                }
                ws.close().await.drop_result();
                debug!("Closed WebSocket connection.");
            })
        });

    let build_artifact = warp::get()
        // Check if the path has a patch.
        .and(warp::path::full())
        .and(warp::get().map(move || (patch_registry_ref.clone(), rebuilder_ref.clone())))
        .and_then(filter_patched_path)
        .untuple_one()
        .and(warp::fs::dir(build_dir.clone()));

    let no_copy_files_except_ext = warp::path::full()
        .and_then(move |full_path: FullPath| async move {
            match full_path.as_str().ends_with(".md") {
                true => Err(warp::reject::not_found()),
                false => Ok(()),
            }
        })
        .untuple_one()
        .and(warp::fs::dir(src_dir));

    // The fallback route for 404 errors
    let fallback_route = warp::fs::file(file_404)
        .map(|reply| warp::reply::with_status(reply, warp::http::StatusCode::NOT_FOUND));
    let routes = live_patch
        .or(build_artifact)
        .or(live_patch_script_filter())
        // Fall back to the source directory for assets.
        .or(no_copy_files_except_ext)
        .or(fallback_route);

    warp::serve(routes).try_bind(address).await;
}

/// Handle live patching at the canonical `path` that may start with `/`,
/// via the WebSocket `ws`.
async fn handle_ws(
    path: &str,
    ws: &mut WebSocket,
    patch_registry_ref: ActorRef<PatchRegistry>,
) -> Result<()> {
    let path = Path::new(path.trim_start_matches('/'));
    info!(?path, "WebSocket connection.");

    let response = patch_registry_ref
        .call(PatchRegistryQuery::Watch(path.to_owned()))
        .await;
    let Ok(PatchRegistryResponse::WatchReceiver(mut watch_receiver)) = response else {
        bail!("Unexpected response calling PatchRegistry: {response:?}.");
    };

    if !watch_receiver.borrow_and_update().is_empty() {
        // Send the existing patch.
        watch_receiver.mark_changed();
    }
    while watch_receiver.changed().await.is_ok() {
        let patch = { watch_receiver.borrow_and_update().clone() };
        if let Err(err) = ws.send(Message::text(patch)).await {
            info!(
                ?err,
                ?path,
                "Patch update did not deliver. Closing WebSocket."
            );
            return Ok(());
        }
        debug!("Sent patch update to WebSocket at {path:?}.");
    }
    Ok(())
}

async fn filter_patched_path(
    full_path: FullPath,
    (patch_registry_ref, rebuilder_ref): (ActorRef<PatchRegistry>, ActorRef<Rebuilder>),
) -> Result<(), warp::reject::Rejection> {
    let path = full_path.as_str().trim_start_matches('/');
    match patch_registry_ref
        .call(PatchRegistryQuery::GetHasPatch(path.into()))
        .await
    {
        Ok(PatchRegistryResponse::HasPatch(has_patch)) => {
            if has_patch {
                debug!(
                    path,
                    "Client requested patched path. Issuing a full rebuild."
                );
                rebuilder_ref
                    .cast(RebuildInfo::Rebuild(false))
                    .await
                    .drop_result();
            }
        }
        response => error!(?response, "Unexpected response calling PatchRegistry"),
    }
    Ok(())
}

const JS_CONTENT_TYPE: &str = "application/javascript";

/// URL path to the JavaScript for live patching.
pub const LIVE_PATCH_PATH: &str = "__mdbook_incremental_preview/websocket_live_patch.js";
const LIVE_PATCH_JS: &[u8] = include_bytes!("websocket_live_patch.js");

pub fn live_patch_script_filter() -> BoxedFilter<(WithHeader<&'static [u8]>,)> {
    warp::get()
        .or(warp::head())
        .unify()
        .and(warp::path::full().and_then(move |full_path: FullPath| {
            let is_live_patch = full_path.as_str().trim_start_matches('/') == LIVE_PATCH_PATH;
            async move {
                match is_live_patch {
                    true => Ok(with_header(LIVE_PATCH_JS, "Content-Type", JS_CONTENT_TYPE)),
                    false => Err(warp::reject::not_found()),
                }
            }
        }))
        .boxed()
}
