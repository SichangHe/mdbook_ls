use super::*;

// NOTE: Below is adapted from
// <https://github.com/rust-lang/mdBook/blob/3bdcc0a5a6f3c85dd751350774261dbc357b02bd/src/cmd/watch/native.rs>.

pub fn rebuild_on_change(book: &mut MDBook, post_build: &dyn Fn()) -> Result<()> {
    // Create a channel to receive the events.
    let (tx, rx) = channel();
    let _debouncer_to_keep_watcher_alive = watch_file_changes(book, tx);

    let config_location = book.root.join("book.toml");
    let mut maybe_gitignore = maybe_make_gitignore(&book.root);
    let mut path2book_items = get_path2book_items(book);
    info!(
        ?config_location,
        len_path2book_items = path2book_items.len()
    );

    loop {
        let paths = recv_changed_paths(book, &maybe_gitignore, &rx);
        if !paths.is_empty() {
            info!(?paths, "Directories change");
            let full_rebuild = match &maybe_gitignore {
                Some((_, gitignore_path)) if paths.contains(gitignore_path) => {
                    // Gitignore file changed, update the gitignore and make
                    // a full reload.
                    maybe_gitignore = maybe_make_gitignore(&book.root);
                    debug!("reloaded gitignore");
                    true
                }
                // Config file changed, make a full reload.
                _ if paths.contains(&config_location) => true,
                _ => false,
            };
            debug!(full_rebuild);
            if full_rebuild {
                match MDBook::load(&book.root) {
                    Ok(mut b) => {
                        if let Err(err) = config_and_build_book(&mut b) {
                            error!(?err, "failed to build the book");
                        }
                        drop(path2book_items);
                        *book = b;
                        path2book_items = get_path2book_items(book);
                        info!("rebuilt the book");
                    }
                    Err(err) => error!(?err, "failed to load book config"),
                }
            } else {
                let paths_w_book_item = paths.iter().filter_map(|path| {
                    path2book_items
                        .get(path)
                        .map(|book_item| (path, *book_item))
                });
                // TODO: Shananigans to avoid a full rebuild.
                for (path, book_item) in paths_w_book_item {
                    let BookItem::Chapter(chapter) = book_item else {
                        bail!("{book_item:?} should not have any associated path.");
                    };
                    debug!(?path, ?chapter.name);
                    // TODO: Patch the rendering in-place.
                }
            }
            post_build();
        }
    }
}

/// Absolute paths of source files to book items.
fn get_path2book_items(book: &MDBook) -> HashMap<PathBuf, &BookItem> {
    book.iter()
        .filter_map(|book_item| match book_item {
            BookItem::Chapter(Chapter { source_path, .. }) => source_path
                .as_ref()
                .map(|source_path| (book.source_dir().join(source_path), book_item)),
            BookItem::Separator | BookItem::PartTitle(_) => None,
        })
        .collect()
}
