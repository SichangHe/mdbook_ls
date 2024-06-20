use super::*;

// NOTE: Below is adapted from
// <https://github.com/rust-lang/mdBook/blob/3bdcc0a5a6f3c85dd751350774261dbc357b02bd/src/cmd/watch/native.rs>.

pub async fn rebuild_on_change(
    book_root: PathBuf,
    serving_url: String,
    build_dir: &Path,
    info_tx: tokio::sync::mpsc::Sender<ServeInfo>,
    mut open_browser: bool,
    post_build: &dyn Fn(),
) -> Result<()> {
    let config_location = book_root.join("book.toml");
    // Create a channel to receive the events.
    let (tx, rx) = channel();

    let mut _debouncer_to_keep_watcher_alive;
    let (mut render_context, mut html_config, mut theme);
    let (mut book, mut src_dir, mut file_404, mut maybe_gitignore) = Default::default();
    let (mut theme_dir, mut handlebars, mut rendering) = Default::default();
    let (mut full_rebuild, mut reload_watcher, mut reload_server) = (true, true, true);
    yield_now().await;

    loop {
        if mem::take(&mut full_rebuild) {
            info!(?build_dir, "Full rebuild");
            match block_in_place(|| MDBook::load(&book_root)) {
                Ok(mut b) => {
                    if let Err(err) = config_book_for_live_reload(&mut b) {
                        error!(?err, "configuring the book for live reload");
                    }
                    yield_now().await;
                    book = b;

                    drop(rendering); // Needed to reassign `render_context`.
                    drop(handlebars);
                    render_context = make_render_context(&book, build_dir)?;
                    (html_config, theme_dir, theme, handlebars) = block_in_place(|| {
                        html_config_n_theme_dir_n_theme_n_handlebars(&render_context)
                    })?;
                    rendering = block_in_place(|| {
                        StatefulHtmlHbs::render(&render_context, html_config, &theme, &handlebars)
                    })?;

                    info!(
                        ?theme_dir,
                        len_rendering_path2ctxs = rendering.path2ctxs.len(),
                        "rebuilt the book"
                    );
                }
                Err(err) => error!(?err, "failed to load book config"),
            }
            post_build();
        }
        if mem::take(&mut reload_watcher) {
            // TODO: Decide if this reload is needed in a finer grained sense.
            info!(?book_root, "Reloading the watcher");
            _debouncer_to_keep_watcher_alive =
                block_in_place(|| watch_file_changes(&book, tx.clone()));
        }
        if mem::take(&mut reload_server) {
            let new_src_dir = book.source_dir();
            let input_404 = book
                .config
                .get("output.html.input-404")
                .and_then(toml::Value::as_str)
                .map(ToString::to_string);
            let new_file_404 = build_dir.join(get_404_output_file(&input_404));
            yield_now().await;

            if (new_src_dir != src_dir) || (new_file_404 != file_404) {
                (src_dir, file_404) = (new_src_dir, new_file_404);
                info!(?src_dir, ?file_404, "Reloading the server");
                info_tx
                    .send(ServeInfo {
                        src_dir: src_dir.clone(),
                        file_404: file_404.clone(),
                    })
                    .await
                    .context("The server is unavailable to receive info.")?;
                if mem::take(&mut open_browser) {
                    block_in_place(|| open(&serving_url));
                }
            }
        }
        // TODO: Use Tokio channel.
        let paths = block_in_place(|| recv_changed_paths(&book_root, &maybe_gitignore, &rx));
        if !paths.is_empty() {
            info!(?paths, "Directories changed");
            // TODO: Watch `SUMMARY.md`.
            (full_rebuild, reload_watcher, reload_server) = match &maybe_gitignore {
                Some((_, gitignore_path)) if paths.contains(gitignore_path) => {
                    // Gitignore file changed,
                    // update the gitignore and make a full rebuild.
                    maybe_gitignore = block_in_place(|| maybe_make_gitignore(&book_root));
                    debug!("reloaded gitignore");
                    (true, false, false)
                }
                // Config file changed, make a full rebuild,
                // reload the watcher and the server.
                _ if paths.contains(&config_location) => (true, true, true),
                // Theme changed, make a full rebuild.
                _ if paths.iter().any(|path| path.starts_with(&theme_dir)) => (true, false, false),
                _ => (false, false, false),
            };
            debug!(full_rebuild, reload_watcher, reload_server);

            if !full_rebuild {
                match block_in_place(|| rendering.patch(&mut book, &src_dir, &paths)) {
                    Ok(_) => post_build(),
                    Err(err) => {
                        error!(?err, "patching the book. Falling back to a full rebuild.");
                        full_rebuild = true;
                    }
                }
            }
        }
    }
}
