use super::*;

// NOTE: Below is adapted from
// <https://github.com/rust-lang/mdBook/blob/3bdcc0a5a6f3c85dd751350774261dbc357b02bd/src/cmd/watch/native.rs>.

pub fn rebuild_on_change(book: &mut MDBook, post_build: &dyn Fn()) {
    // Create a channel to receive the events.
    let (tx, rx) = channel();
    let _debouncer_to_keep_watcher_alive = watch_file_changes(book, tx);

    let config_location = book.root.join("book.toml");
    let maybe_gitignore = maybe_make_gitignore(&book.root);
    info!(?config_location);
    loop {
        let events = recv_changed_paths(book, &maybe_gitignore, &rx);
        if !events.is_empty() {
            info!(?events, "File change events");
            /*
            if events.contains(&config_location) {
                // TODO: Leverage this info to avoid full rebuilds.
                // The configuration changed, perform a full rebuild.
            }
            */
            match MDBook::load(&book.root) {
                Ok(mut b) => {
                    match config_and_build_book(&mut b) {
                        Ok(()) => post_build(),
                        Err(err) => error!(?err, "failed to build the book"),
                    }
                    *book = b;
                    info!("rebuilt the book");
                }
                Err(err) => error!(?err, "failed to load book config"),
            }
        }
    }
}
