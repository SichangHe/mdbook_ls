use super::*;

pub async fn serve_reloading(
    book_root: PathBuf,
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
        let maybe_maybe_info = select! {
            _ = serve(book_root.clone(), build_dir.clone(), address, reload_tx.clone(), info.clone()) => None,
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
    pub theme_dir: PathBuf,
    pub additional_css: Vec<PathBuf>,
    pub additional_js: Vec<PathBuf>,
    pub file_404: PathBuf,
}

pub async fn serve(
    book_root: PathBuf,
    build_dir: PathBuf,
    address: SocketAddr,
    reload_tx: broadcast::Sender<Message>,
    info: ServeInfo,
) {
    let ServeInfo {
        src_dir,
        theme_dir,
        additional_css,
        additional_js,
        file_404,
    } = info;

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

    // Serve artifacts in the build directory.
    let book_route = warp::fs::dir(build_dir.clone());
    let no_copy_static_files = warp::fs::dir(theme_dir).or(static_files_filter());
    let no_copy_additional_css_and_js =
        additional_js_css_filter(book_root, &additional_js, &additional_css);

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
    let routes = livereload
        .or(book_route)
        .or(no_copy_static_files)
        .or(no_copy_additional_css_and_js)
        // Fall back to the source directory for assets.
        .or(no_copy_files_except_ext)
        .or(fallback_route);

    std::panic::set_hook(Box::new(move |panic_info| {
        // exit if serve panics
        error!("Unable to serve: {}", panic_info);
        std::process::exit(1);
    }));

    warp::serve(routes).run(address).await;
}

const CONTENT_TYPE: &str = "Content-Type";
const JS_CONTENT_TYPE: &str = "application/javascript";
const CSS_CONTENT_TYPE: &str = "text/css";
const TTF_CONTENT_TYPE: &str = "font/ttf";
const SVG_CONTENT_TYPE: &str = "image/svg+xml";
const WOFF2_CONTENT_TYPE: &str = "font/woff2";
const TXT_CONTENT_TYPE: &str = "text/plain";

/// Mirror the content and order in `HtmlHandlebars::copy_static_files` but
/// serve them directly instead of copying.
///
/// `.nojekyll` and `CNAME` are not included.
pub fn static_files_filter() -> BoxedFilter<(WithHeader<&'static [u8]>,)> {
    let path2content_n_types: HashMap<&'static str, (&'static [u8], &'static str)> =
        HashMap::from_iter(
            [
                // Fallback theme.
                ("book.js", (theme::JS, JS_CONTENT_TYPE)),
                ("css/chrome.css", (theme::CHROME_CSS, CSS_CONTENT_TYPE)),
                ("css/general.css", (theme::GENERAL_CSS, CSS_CONTENT_TYPE)),
                ("css/print.css", (theme::PRINT_CSS, CSS_CONTENT_TYPE)),
                (
                    "css/variables.css",
                    (theme::VARIABLES_CSS, CSS_CONTENT_TYPE),
                ),
                ("favicon.png", (theme::FAVICON_PNG, "image/png")),
                ("favicon.svg", (theme::FAVICON_SVG, SVG_CONTENT_TYPE)),
                ("highlight.css", (theme::HIGHLIGHT_CSS, CSS_CONTENT_TYPE)),
                (
                    "tomorrow-night.css",
                    (theme::TOMORROW_NIGHT_CSS, CSS_CONTENT_TYPE),
                ),
                (
                    "ayu-highlight.css",
                    (theme::AYU_HIGHLIGHT_CSS, CSS_CONTENT_TYPE),
                ),
                ("highlight.js", (theme::HIGHLIGHT_JS, JS_CONTENT_TYPE)),
                ("clipboard.min.js", (theme::CLIPBOARD_JS, JS_CONTENT_TYPE)),
                // Font Awesome.
                (
                    "FontAwesome/css/font-awesome.css",
                    (theme::FONT_AWESOME, CSS_CONTENT_TYPE),
                ),
                (
                    "FontAwesome/fonts/fontawesome-webfont.eot",
                    (theme::FONT_AWESOME_EOT, "application/vnd.ms-fontobject"),
                ),
                (
                    "FontAwesome/fonts/fontawesome-webfont.svg",
                    (theme::FONT_AWESOME_SVG, SVG_CONTENT_TYPE),
                ),
                (
                    "FontAwesome/fonts/fontawesome-webfont.ttf",
                    (theme::FONT_AWESOME_TTF, TTF_CONTENT_TYPE),
                ),
                (
                    "FontAwesome/fonts/fontawesome-webfont.woff",
                    (theme::FONT_AWESOME_WOFF, "font/woff"),
                ),
                (
                    "FontAwesome/fonts/fontawesome-webfont.woff2",
                    (theme::FONT_AWESOME_WOFF2, WOFF2_CONTENT_TYPE),
                ),
                (
                    "FontAwesome/fonts/FontAwesome.ttf",
                    (theme::FONT_AWESOME_TTF, TTF_CONTENT_TYPE),
                ),
                // Fallback font.
                ("fonts/fonts.css", (theme::fonts::CSS, CSS_CONTENT_TYPE)),
                // Playground.
                ("editor.js", (playground_editor::JS, JS_CONTENT_TYPE)),
                ("ace.js", (playground_editor::ACE_JS, JS_CONTENT_TYPE)),
                (
                    "mode-rust.js",
                    (playground_editor::MODE_RUST_JS, JS_CONTENT_TYPE),
                ),
                (
                    "theme-dawn.js",
                    (playground_editor::THEME_DAWN_JS, JS_CONTENT_TYPE),
                ),
                (
                    "theme-tomorrow_night.js",
                    (playground_editor::THEME_TOMORROW_NIGHT_JS, JS_CONTENT_TYPE),
                ),
            ]
            .into_iter()
            // Other fallback fonts.
            .chain(
                theme::fonts::LICENSES
                    .into_iter()
                    .map(|(path, contents)| (path, (contents, TXT_CONTENT_TYPE))),
            )
            .chain(
                theme::fonts::OPEN_SANS
                    .into_iter()
                    .chain(iter::once(theme::fonts::SOURCE_CODE_PRO))
                    .map(|(path, contents)| (path, (contents, WOFF2_CONTENT_TYPE))),
            ),
        );

    warp::get()
        .or(warp::head())
        .unify()
        .and(warp::path::full().and_then(move |full_path: FullPath| {
            let maybe_content_n_type =
                path2content_n_types.get(full_path.as_str().trim_start_matches('/'));
            let result = match maybe_content_n_type {
                Some((content, content_type)) => {
                    Ok(with_header(*content, CONTENT_TYPE, *content_type))
                }
                None => Err(warp::reject::not_found()),
            };
            async { result }
        }))
        .boxed()
}

/// Mirror `HtmlHandlebars::copy_additional_css_and_js` but
/// serve them directly instead of copying.
pub fn additional_js_css_filter(
    book_root: PathBuf,
    additional_js: &[PathBuf],
    additional_css: &[PathBuf],
) -> BoxedFilter<(warp::fs::File,)> {
    let additional_paths = additional_js
        .iter()
        .chain(additional_css)
        .map(|path| path.display().to_string())
        .collect::<HashSet<_>>();
    debug!(?additional_paths);
    warp::path::full()
        .and_then(move |full_path: FullPath| {
            let is_additional_path =
                additional_paths.contains(full_path.as_str().trim_start_matches('/'));
            trace!(?full_path, ?is_additional_path, "Checking additional paths");
            async move {
                match is_additional_path {
                    true => Ok(()),
                    false => Err(warp::reject::not_found()),
                }
            }
        })
        .untuple_one()
        .and(warp::fs::dir(book_root))
        .boxed()
}
