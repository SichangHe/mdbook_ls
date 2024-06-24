use super::*;

// NOTE: Below is adapted from
// <https://github.com/rust-lang/mdBook/blob/3bdcc0a5a6f3c85dd751350774261dbc357b02bd/src/cmd/watch/native.rs>.

pub async fn rebuild_on_change(
    book_root: PathBuf,
    mut serving_url: Option<String>,
    build_dir: &Path,
    info_tx: mpsc::Sender<ServeInfo>,
    file_event_tx: mpsc::Sender<Vec<PathBuf>>,
    mut file_event_rx: mpsc::Receiver<Vec<PathBuf>>,
    mut patch_registry_ref: ActorRef<PatchRegistry>,
) -> Result<()> {
    let book_toml = book_root.join("book.toml");

    let mut _debouncer_to_keep_watcher_alive;
    let (mut book, mut file_404, mut maybe_gitignore, mut summary_md) = Default::default();
    let (mut theme_dir, mut html_config, mut old_html_config) = Default::default();
    let (mut src_dir, mut extra_watch_dirs, mut hbs_state): (PathBuf, Vec<_>, HtmlHbsState) =
        Default::default();
    let (mut full_rebuild, mut reload_watcher, mut reload_server) = (true, true, true);
    yield_now().await;

    loop {
        let (mut src_dir_changed, mut theme_dir_changed) = (None, false);
        if mem::take(&mut full_rebuild) {
            info!(?build_dir, "Full rebuild");
            match block_in_place(|| MDBook::load(&book_root)) {
                Ok(mut b) => {
                    if let Err(err) = config_book_for_live_reload(&mut b) {
                        error!(?err, "configuring the book for live reload");
                    }
                    yield_now().await;
                    book = b;

                    let render_context = make_render_context(&book, build_dir)?;
                    let old_theme_dir = theme_dir;
                    old_html_config = html_config;
                    let (theme, handlebars);
                    (html_config, theme_dir, theme, handlebars) = block_in_place(|| {
                        html_config_n_theme_dir_n_theme_n_handlebars(&render_context)
                    })?;
                    theme_dir_changed = old_theme_dir != theme_dir;
                    hbs_state
                        .full_render(&render_context, html_config.clone(), &theme, &handlebars)
                        .await?;

                    info!(
                        ?theme_dir,
                        len_rendering_path2ctxs = hbs_state.path2ctxs.len(),
                        ?hbs_state.index_path,
                        "rebuilt the book"
                    );
                    patch_registry_ref
                        .cast(PatchRegistryRequest::Clear {
                            index_path: hbs_state.index_path.clone(),
                            smart_punctuation: hbs_state.smart_punctuation,
                        })
                        .await
                        .context("Clearing the patch registry")?;
                }
                Err(err) => error!(?err, "failed to load book config"),
            }
        }
        if mem::take(&mut reload_watcher) {
            let new_src_dir = book.source_dir();
            src_dir_changed = Some(match new_src_dir == src_dir {
                false => {
                    src_dir = new_src_dir;
                    summary_md = src_dir.join("SUMMARY.md");
                    true
                }
                true => false,
            });

            let extra_watch_dirs_changed =
                match extra_watch_dirs == book.config.build.extra_watch_dirs {
                    false => {
                        extra_watch_dirs.clone_from(&book.config.build.extra_watch_dirs);
                        true
                    }
                    true => false,
                };

            debug!(
                ?src_dir_changed,
                theme_dir_changed, extra_watch_dirs_changed
            );
            if src_dir_changed == Some(true) || theme_dir_changed || extra_watch_dirs_changed {
                info!(?book_root, "Reloading the watcher");
                let tx = file_event_tx.clone();
                let event_handler = move |events: Result<Vec<DebouncedEvent>, _>| match events {
                    Ok(events) => tx
                        .blocking_send(events.into_iter().map(|event| event.path).collect())
                        .drop_result(),
                    Err(err) => error!(?err, "Watching for changes"),
                };
                _debouncer_to_keep_watcher_alive = block_in_place(|| {
                    watch_file_changes(
                        &book_root,
                        &src_dir,
                        &theme_dir,
                        &book_toml,
                        &extra_watch_dirs,
                        event_handler,
                    )
                });
            }
        }
        if mem::take(&mut reload_server) {
            let src_dir_changed = src_dir_changed.unwrap_or_else(|| {
                let new_src_dir = book.source_dir();
                match new_src_dir == src_dir {
                    false => {
                        src_dir = new_src_dir;
                        summary_md = src_dir.join("SUMMARY.md");
                        true
                    }
                    true => false,
                }
            });

            let input_404 = book
                .config
                .get("output.html.input-404")
                .and_then(toml::Value::as_str)
                .map(ToString::to_string);
            let new_file_404 = build_dir.join(get_404_output_file(&input_404));
            yield_now().await;
            let file_404_changed = match new_file_404 == file_404 {
                false => {
                    file_404 = new_file_404;
                    true
                }
                true => false,
            };

            debug!(src_dir_changed, file_404_changed);
            if src_dir_changed
                || html_config.additional_js != old_html_config.additional_js
                || html_config.additional_css != old_html_config.additional_css
                || file_404_changed
            {
                info!(?src_dir, ?html_config.additional_js, ?html_config.additional_css, ?file_404, "Reloading the server");
                info_tx
                    .send(ServeInfo {
                        src_dir: src_dir.clone(),
                        theme_dir: theme_dir.clone(),
                        additional_js: html_config.additional_js.clone(),
                        additional_css: html_config.additional_css.clone(),
                        file_404: file_404.clone(),
                    })
                    .await
                    .context("The server is unavailable to receive info.")?;
                if let Some(serving_url) = mem::take(&mut serving_url) {
                    block_in_place(|| open(&serving_url));
                }
            }
        }
        let paths = recv_changed_paths(&book_root, &maybe_gitignore, &mut file_event_rx).await;
        if !paths.is_empty() {
            info!(?paths, "Directories changed");
            (full_rebuild, reload_watcher, reload_server) = match &maybe_gitignore {
                Some((_, gitignore_path)) if paths.contains(gitignore_path) => {
                    // Gitignore file changed,
                    // update the gitignore and make a full rebuild.
                    maybe_gitignore = block_in_place(|| maybe_make_gitignore(&book_root));
                    debug!("reloaded gitignore");
                    (true, false, false)
                }
                // `book.toml` changed, make a full rebuild,
                // reload the watcher and the server.
                _ if paths.contains(&book_toml) => (true, true, true),
                // `Summary.md` or theme changed, make a full rebuild.
                _ if paths.contains(&summary_md)
                    || paths.iter().any(|path| path.starts_with(&theme_dir)) =>
                {
                    (true, false, false)
                }
                _ => (false, false, false),
            };
            debug!(full_rebuild, reload_watcher, reload_server);

            if !full_rebuild {
                match hbs_state
                    .patch(&mut book, &src_dir, &paths, &mut patch_registry_ref)
                    .await
                {
                    Ok(_) => debug!("Patched the book"),
                    Err(err) => {
                        error!(?err, "patching the book. Falling back to a full rebuild.");
                        full_rebuild = true;
                    }
                }
            }
        }
    }
}
