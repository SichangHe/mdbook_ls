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
            _ = serve(&book_root, build_dir.clone(), address, reload_tx.clone(), info.clone()) => None,
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

const CONTENT_TYPE: &str = "Content-Type";
const JS_CONTENT_TYPE: &str = "application/javascript";
const CSS_CONTENT_TYPE: &str = "text/css";
const TTF_CONTENT_TYPE: &str = "font/ttf";
const SVG_CONTENT_TYPE: &str = "image/svg+xml";
const WOFF2_CONTENT_TYPE: &str = "font/woff2";
const TXT_CONTENT_TYPE: &str = "text/plain";

pub async fn serve(
    book_root: &Path,
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

    // NOTE: Mirror the content and order in `HtmlHandlebars::copy_static_files`
    // but serve them directly instead of copying.
    // `.nojekyll` and `CNAME` are not included.
    //
    // The fact that this is not extracted into a function is because
    // I have not figured out how to box it to name the type.
    let no_copy_static_files = warp::fs::dir(theme_dir)
        // Fallback theme.
        .or(full_path("book.js").map(|| with_header(theme::JS, CONTENT_TYPE, JS_CONTENT_TYPE)))
        .or(full_path("css/chrome.css")
            .map(|| with_header(theme::CHROME_CSS, CONTENT_TYPE, CSS_CONTENT_TYPE)))
        .or(full_path("css/general.css")
            .map(|| with_header(theme::GENERAL_CSS, CONTENT_TYPE, CSS_CONTENT_TYPE)))
        .or(full_path("css/print.css")
            .map(|| with_header(theme::PRINT_CSS, CONTENT_TYPE, CSS_CONTENT_TYPE)))
        .or(full_path("css/variables.css")
            .map(|| with_header(theme::VARIABLES_CSS, CONTENT_TYPE, CSS_CONTENT_TYPE)))
        .or(full_path("favicon.png")
            .map(|| with_header(theme::FAVICON_PNG, CONTENT_TYPE, "image/png")))
        .or(full_path("favicon.svg")
            .map(|| with_header(theme::FAVICON_SVG, CONTENT_TYPE, SVG_CONTENT_TYPE)))
        .or(full_path("highlight.css")
            .map(|| with_header(theme::HIGHLIGHT_CSS, CONTENT_TYPE, CSS_CONTENT_TYPE)))
        .or(full_path("tomorrow-night.css")
            .map(|| with_header(theme::TOMORROW_NIGHT_CSS, CONTENT_TYPE, CSS_CONTENT_TYPE)))
        .or(full_path("ayu-highlight.css")
            .map(|| with_header(theme::AYU_HIGHLIGHT_CSS, CONTENT_TYPE, CSS_CONTENT_TYPE)))
        .or(full_path("highlight.js")
            .map(|| with_header(theme::HIGHLIGHT_JS, CONTENT_TYPE, JS_CONTENT_TYPE)))
        .or(full_path("clipboard.min.js")
            .map(|| with_header(theme::CLIPBOARD_JS, CONTENT_TYPE, JS_CONTENT_TYPE)))
        // Font Awesome.
        .or(full_path("FontAwesome/css/font-awesome.css")
            .map(|| with_header(theme::FONT_AWESOME, CONTENT_TYPE, CSS_CONTENT_TYPE)))
        .or(
            full_path("FontAwesome/fonts/fontawesome-webfont.eot").map(|| {
                with_header(
                    theme::FONT_AWESOME_EOT,
                    CONTENT_TYPE,
                    "application/vnd.ms-fontobject",
                )
            }),
        )
        .or(full_path("FontAwesome/fonts/fontawesome-webfont.svg")
            .map(|| with_header(theme::FONT_AWESOME_SVG, CONTENT_TYPE, SVG_CONTENT_TYPE)))
        .or(full_path("FontAwesome/fonts/fontawesome-webfont.ttf")
            .map(|| with_header(theme::FONT_AWESOME_TTF, CONTENT_TYPE, TTF_CONTENT_TYPE)))
        .or(full_path("FontAwesome/fonts/fontawesome-webfont.woff")
            .map(|| with_header(theme::FONT_AWESOME_WOFF, CONTENT_TYPE, "font/woff")))
        .or(full_path("FontAwesome/fonts/fontawesome-webfont.woff2")
            .map(|| with_header(theme::FONT_AWESOME_WOFF2, CONTENT_TYPE, WOFF2_CONTENT_TYPE)))
        .or(full_path("FontAwesome/fonts/FontAwesome.ttf")
            .map(|| with_header(theme::FONT_AWESOME_TTF, CONTENT_TYPE, TTF_CONTENT_TYPE)))
        // Fallback fonts.
        .or(full_path("fonts/fonts.css")
            .map(|| with_header(theme::fonts::CSS, CONTENT_TYPE, CSS_CONTENT_TYPE)))
        .or(theme::fonts::LICENSES
            .into_iter()
            .map(|(path, contents)| {
                full_path(path)
                    .map(move || with_header(contents, CONTENT_TYPE, TXT_CONTENT_TYPE))
                    .boxed()
            })
            .reduce(|a, b| a.or(b).unify().boxed())
            .expect("not empty"))
        .or(theme::fonts::OPEN_SANS
            .into_iter()
            .map(|(path, contents)| {
                full_path(path)
                    .map(move || with_header(contents, CONTENT_TYPE, WOFF2_CONTENT_TYPE))
                    .boxed()
            })
            .reduce(|a, b| a.or(b).unify().boxed())
            .expect("not empty"))
        .or(full_path(theme::fonts::SOURCE_CODE_PRO.0).map(|| {
            with_header(
                theme::fonts::SOURCE_CODE_PRO.1,
                CONTENT_TYPE,
                WOFF2_CONTENT_TYPE,
            )
        }))
        // Playground.
        .or(full_path("editor.js")
            .map(|| with_header(playground_editor::JS, CONTENT_TYPE, JS_CONTENT_TYPE)))
        .or(full_path("ace.js")
            .map(|| with_header(playground_editor::ACE_JS, CONTENT_TYPE, JS_CONTENT_TYPE)))
        .or(full_path("mode-rust.js").map(|| {
            with_header(
                playground_editor::MODE_RUST_JS,
                CONTENT_TYPE,
                JS_CONTENT_TYPE,
            )
        }))
        .or(full_path("theme-dawn.js").map(|| {
            with_header(
                playground_editor::THEME_DAWN_JS,
                CONTENT_TYPE,
                JS_CONTENT_TYPE,
            )
        }))
        .or(full_path("theme-tomorrow_night.js").map(|| {
            with_header(
                playground_editor::THEME_TOMORROW_NIGHT_JS,
                CONTENT_TYPE,
                JS_CONTENT_TYPE,
            )
        }));

    // NOTE: Mirror `HtmlHandlebars::copy_additional_css_and_js` but
    // serve them directly instead of copying.
    let no_copy_additional_css_and_js = additional_js
        .iter()
        .chain(&additional_css)
        .map(|path| {
            full_path(&format!("{path:?}"))
                .and(warp::fs::file(book_root.join(path)))
                .boxed()
        })
        .reduce(|a, b| a.or(b).unify().boxed())
        .unwrap_or_else(|| warp::fs::file("does_not_exist").boxed());

    // The fallback route for 404 errors
    let fallback_route = warp::fs::file(file_404)
        .map(|reply| warp::reply::with_status(reply, warp::http::StatusCode::NOT_FOUND));
    let routes = livereload
        .or(book_route)
        .or(no_copy_static_files)
        .or(no_copy_additional_css_and_js)
        // Fall back to the source directory for assets.
        .or(warp::fs::dir(src_dir))
        .or(fallback_route);

    std::panic::set_hook(Box::new(move |panic_info| {
        // exit if serve panics
        error!("Unable to serve: {}", panic_info);
        std::process::exit(1);
    }));

    warp::serve(routes).run(address).await;
}

/// Unlike [`warp::path`], handles `/`s in `path`.
pub fn full_path(path: &str) -> BoxedFilter<()> {
    path.split('/')
        .map(|segment| warp::path(segment.to_owned()).boxed())
        .reduce(|a, b| a.and(b).boxed())
        .map(|f| f.and(warp::path::end()).boxed())
        .unwrap_or_else(|| warp::path::end().boxed())
}
