use super::*;

// NOTE: Below is adapted from
// <https://github.com/rust-lang/mdBook/blob/3bdcc0a5a6f3c85dd751350774261dbc357b02bd/src/cmd/watch/native.rs>.

pub fn rebuild_on_change(
    book: &mut MDBook,
    ready: Arc<Barrier>,
    post_build: &dyn Fn(),
) -> Result<()> {
    // Create a channel to receive the events.
    let (tx, rx) = channel();
    let _debouncer_to_keep_watcher_alive = watch_file_changes(book, tx);

    let book_root = book.root.clone();
    let config_location = book_root.join("book.toml");
    let mut maybe_gitignore = maybe_make_gitignore(&book_root);
    config_book_for_live_reload(book)?;
    let mut render_context = make_render_context(book)?;
    let (mut html_config, mut theme, mut handlebars) =
        make_html_config_theme_and_handlebars(&render_context)?;
    let mut rendering = StatefulHtmlHbs::render(&render_context, html_config, &theme, &handlebars)?;
    ready.wait(); // Notify that the book is built.
    info!(
        ?config_location,
        len_rendering_path2ctxs = rendering.path2ctxs.len()
    );

    let mut full_rebuild = false;
    let mut paths;

    loop {
        if full_rebuild {
            match MDBook::load(&book_root) {
                Ok(mut b) => {
                    if let Err(err) = config_book_for_live_reload(&mut b) {
                        error!(?err, "configuring the book for live reload");
                    }

                    drop(rendering); // Needed to reassign `render_context`.
                    drop(handlebars);
                    render_context = make_render_context(book)?;
                    (html_config, theme, handlebars) =
                        make_html_config_theme_and_handlebars(&render_context)?;
                    rendering =
                        StatefulHtmlHbs::render(&render_context, html_config, &theme, &handlebars)?;

                    *book = b;
                    info!("rebuilt the book");
                }
                Err(err) => error!(?err, "failed to load book config"),
            }
            post_build();
        }
        paths = recv_changed_paths(&book_root, &maybe_gitignore, &rx);
        if !paths.is_empty() {
            info!(?paths, "Directories change");
            full_rebuild = match &maybe_gitignore {
                Some((_, gitignore_path)) if paths.contains(gitignore_path) => {
                    // Gitignore file changed, update the gitignore and make
                    // a full reload.
                    maybe_gitignore = maybe_make_gitignore(&book_root);
                    debug!("reloaded gitignore");
                    true
                }
                // Config file changed, make a full reload.
                _ if paths.contains(&config_location) => true,
                _ => false,
            };
            debug!(full_rebuild);
            if !full_rebuild {
                match rendering.patch(book, &paths) {
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
