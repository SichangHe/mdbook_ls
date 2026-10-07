use super::*;

pub async fn serve_reloading(
    address: SocketAddr,
    build_dir: PathBuf,
    mut info_rx: mpsc::Receiver<ServeInfo>,
    patch_registry_ref: ActorRef<PatchRegistry>,
    reverse_search: Option<mpsc::Sender<SearchLocation>>,
    forward_search: broadcast::Sender<String>,
) {
    let Some(mut info) = info_rx.recv().await else {
        error!("Did not start server because all info senders have been dropped.");
        return;
    };
    info!("Starting server with reloading.");
    let mut info_buf = Vec::new();
    loop {
        let maybe_maybe_info = select! {
            _ = serve(build_dir.clone(), address, info.clone(), patch_registry_ref.clone(), reverse_search.clone(), forward_search.clone()) => None,
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
    build_dir: PathBuf,
    address: SocketAddr,
    info: ServeInfo,
    patch_registry_ref: ActorRef<PatchRegistry>,
    reverse_search: Option<mpsc::Sender<SearchLocation>>,
    forward_search: broadcast::Sender<String>,
) {
    let ServeInfo { src_dir, file_404 } = info;
    let search_src = src_dir.clone();
    let search_registry = patch_registry_ref.clone();
    let search = warp::path("__mdbook_source_search")
        .and(warp::path::end())
        .and(warp::header::optional::<String>("origin"))
        .and(warp::header::<String>("host"))
        .and_then(|origin: Option<String>, host: String| async move {
            match origin {
                Some(origin) if origin == format!("http://{host}") => Ok(()),
                _ => Err(warp::reject::not_found()),
            }
        })
        .untuple_one()
        .and(warp::ws())
        .map(move |ws: Ws| {
            let registry = search_registry.clone();
            let (sender, mut forward, src) = (reverse_search.clone(), forward_search.subscribe(), search_src.clone());
            ws.on_upgrade(move |mut ws| async move {
                loop {
                    select! {
                        value = forward.recv() => {
                            let value = match value {
                                Ok(value) => value,
                                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                                Err(broadcast::error::RecvError::Closed) => break,
                            };
                            if ws.send(Message::text(value)).await.is_err() { break; }
                        }
                        message = ws.next() => {
                            let Some(Ok(message)) = message else { break; };
                            let Ok(text) = message.to_str() else { continue; };
                            let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else { continue; };
                            let (Some(path), Some(line)) = (value["path"].as_str(), value["line"].as_u64()) else { continue; };
                            let Ok(line) = u32::try_from(line) else { continue; };
                            let Ok(path) = std::fs::canonicalize(path) else { continue; };
                            let Ok(root) = std::fs::canonicalize(&src) else { continue; };
                            if path.starts_with(root) && path.extension() == Some(OsStr::new("md")) {
                                if !matches!(registry.call(PatchRegistryQuery::HasSource(path.clone())).await, Ok(PatchRegistryResponse::HasSource(true))) { continue; }
                                if let Some(sender) = &sender {
                                    if sender.send(SearchLocation { path, line, character: 0 }).await.is_err() { break; }
                                }
                            }
                        }
                    }
                }
            })
        });

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

    let patched_dir = build_dir.clone();
    let build_artifact = warp::get()
        .and(warp::path::full())
        .and(warp::any().map(move || (patch_registry_ref.clone(), patched_dir.clone())))
        .and_then(filter_patched_path)
        .or(warp::fs::dir(build_dir.clone()));

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
    let routes = search
        .or(live_patch)
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
    let decoded = decode_chapter_path(path)?;
    let path = decoded.as_path();
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
    (patch_registry_ref, build_dir): (ActorRef<PatchRegistry>, PathBuf),
) -> Result<warp::reply::Html<String>, warp::reject::Rejection> {
    let path = decode_chapter_path(full_path.as_str()).map_err(|_| warp::reject::not_found())?;
    match patch_registry_ref
        .call(PatchRegistryQuery::GetPatch(path.clone()))
        .await
    {
        Ok(PatchRegistryResponse::Patch(Some(patch))) => {
            let relative = if path.as_os_str().is_empty() {
                Path::new("index.html")
            } else {
                &path
            };
            let html = fs::read_to_string(build_dir.join(relative))
                .await
                .map_err(|_| warp::reject::not_found())?;
            let main = extract_main_inner_html(&html).map_err(|_| warp::reject::not_found())?;
            let start = main.as_ptr() as usize - html.as_ptr() as usize;
            let mut merged = html[..start].to_owned();
            merged.push_str(&patch);
            merged.push_str(&html[start + main.len()..]);
            return Ok(warp::reply::html(merged));
        }
        Ok(PatchRegistryResponse::Patch(None)) => {}
        response => error!(?response, "Unexpected response calling PatchRegistry"),
    }
    Err(warp::reject::not_found())
}

fn decode_chapter_path(path: &str) -> Result<PathBuf> {
    let decoded =
        percent_encoding::percent_decode_str(path.trim_start_matches('/')).decode_utf8()?;
    let path = PathBuf::from(decoded.as_ref());
    if path
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        bail!("Invalid chapter URL path");
    }
    Ok(path)
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
