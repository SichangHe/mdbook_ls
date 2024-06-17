use super::*;

const DEBOUNCER_TIMEOUT: Duration = Duration::from_millis(20);

pub(crate) fn watch_file_changes<F>(
    book: &MDBook,
    event_handler: F,
) -> Debouncer<RecommendedWatcher>
where
    F: DebounceEventHandler,
{
    let mut debouncer = match notify_debouncer_mini::new_debouncer(DEBOUNCER_TIMEOUT, event_handler)
    {
        Ok(d) => d,
        Err(err) => {
            error!(?err, "Trying to watch files");
            std::process::exit(1)
        }
    };

    let watcher = debouncer.watcher();

    // Add the source directory to the watcher
    if let Err(err) = watcher.watch(&book.source_dir(), Recursive) {
        error!(source_dir = ?book.source_dir(), ?err, "watching");
        std::process::exit(1);
    };

    let _ = watcher.watch(&book.theme_dir(), Recursive);

    // Add the book.toml file to the watcher if it exists
    let _ = watcher.watch(&book.root.join("book.toml"), NonRecursive);

    for dir in &book.config.build.extra_watch_dirs {
        let path = book.root.join(dir);
        let canonical_path = path.canonicalize().unwrap_or_else(|err| {
            error!(?path, ?err, "Watching extra directory");
            std::process::exit(1);
        });

        if let Err(err) = watcher.watch(&canonical_path, Recursive) {
            error!(?canonical_path, ?err, "Watching extra directory",);
            std::process::exit(1);
        }
    }

    info!("Listening for file changes.");
    debouncer
}

const EVENT_RECEIVE_TIMEOUT: Duration = Duration::from_millis(50);

pub(crate) fn recv_changed_paths(
    book: &MDBook,
    maybe_gitignore: &Option<(Gitignore, PathBuf)>,
    rx: &Receiver<notify::Result<Vec<DebouncedEvent>>>,
) -> Vec<DebouncedEvent> {
    let first_event = rx.recv().unwrap();
    sleep(EVENT_RECEIVE_TIMEOUT);
    let other_events = rx.try_iter();

    std::iter::once(first_event)
        .chain(other_events)
        .filter_map(|maybe_events| match maybe_events {
            Ok(events) => Some(events),
            Err(err) => {
                warn!(?err, "Watching for changes");
                None
            }
        })
        .flatten()
        .filter(|event| {
            let path = &event.path;
            // If we are watching files outside the current repository (via extra-watch-dirs), then they are definitionally
            // ignored by gitignore. So we handle this case by including such files into the watched paths list.
            !path.starts_with(&book.root)
                || match maybe_gitignore {
                    Some((ignore, ignore_root)) => !is_ignored_file(ignore, ignore_root, path),
                    None => true,
                }
        })
        .collect::<Vec<_>>()
}
