use super::*;

const DEBOUNCER_TIMEOUT: Duration = Duration::from_millis(20);

pub fn watch_file_changes<F>(
    book_root: &Path,
    src_dir: &Path,
    theme_dir: &Path,
    book_toml: &Path,
    extra_watch_dirs: &[PathBuf],
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
    if let Err(err) = watcher.watch(src_dir, Recursive) {
        error!(?src_dir, ?err, "watching");
        std::process::exit(1);
    };

    let _ = watcher.watch(theme_dir, Recursive);

    // Add the book.toml file to the watcher if it exists
    let _ = watcher.watch(book_toml, NonRecursive);

    for dir in extra_watch_dirs {
        let path = book_root.join(dir);
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

pub async fn recv_changed_paths<P: AsRef<Path>>(
    book_root: P,
    maybe_gitignore: &Option<(Gitignore, PathBuf)>,
    rx: &mut mpsc::Receiver<Vec<PathBuf>>,
) -> HashSet<PathBuf> {
    let book_root = book_root.as_ref();
    let first_event = rx.recv().await.unwrap();
    let mut other_events = Vec::with_capacity(rx.len() * 2);
    timeout(
        EVENT_RECEIVE_TIMEOUT,
        rx.recv_many(&mut other_events, usize::MAX),
    )
    .await
    .drop_result();

    std::iter::once(first_event)
        .chain(other_events)
        .flatten()
        .filter_map(|path| {
            // If we are watching files outside the current repository (via extra-watch-dirs), then they are definitionally
            // ignored by gitignore. So we handle this case by including such files into the watched paths list.
            match path.starts_with(book_root) {
                true if matches!(
                    maybe_gitignore, Some((ignore, ignore_root)) if is_ignored_file(ignore, ignore_root, &path)
                ) => None,
                _ => Some(path),
            }
        })
        .collect()
}
