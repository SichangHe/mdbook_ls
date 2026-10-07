use std::collections::hash_map::Entry;

use super::*;

/// A registry of watch channel senders of patches for paths.
#[derive(Default)]
pub struct PatchRegistry {
    /// HTML `<main>` body content and watch channel for each patched path.
    patches: HashMap<PathBuf, (String, watch::Sender<String>)>,
    /// Relative rendered HTML path of the index chapter.
    index_path: Option<PathBuf>,
    source_paths: HashSet<PathBuf>,
}

impl Actor for PatchRegistry {
    type Call = PatchRegistryQuery;
    type Cast = PatchRegistryRequest;
    type Reply = PatchRegistryResponse;

    async fn handle_cast(&mut self, msg: Self::Cast, _env: &mut ActorEnv<Self>) -> Result<()> {
        match msg {
            PatchRegistryRequest::NewPatch(path, new_html) => {
                debug!(?path, "Registry received patch.");
                match self.patches.entry(path) {
                    // Entry exists,
                    // update the patch in-place and send watch updates.
                    Entry::Occupied(mut entry) => {
                        let (html, sender) = entry.get_mut();
                        // Update the patch only if it changed.
                        if *html != new_html {
                            debug!("Updating patch in registry.");
                            *html = new_html.clone();
                            sender.send_modify(|html| *html = new_html);
                        }
                    }
                    // New entry, register the patch and a new watch channel.
                    Entry::Vacant(entry) => {
                        _ = entry.insert((new_html.clone(), watch::channel(new_html).0))
                    }
                };
            }
            PatchRegistryRequest::Rebuild {
                index_path,
                source_paths,
            } => {
                self.source_paths = source_paths;
                for (_, (_, watcher)) in self.patches.drain() {
                    watcher.send_modify(|v| *v = "__RELOAD".into())
                }
                self.index_path = index_path;
            }
            PatchRegistryRequest::Clear => self.patches.clear(),
        }
        Ok(())
    }

    async fn handle_call(
        &mut self,
        msg: Self::Call,
        _env: &mut ActorEnv<Self>,
        response_sender: oneshot::Sender<Self::Reply>,
    ) -> Result<()> {
        debug!(?msg, "PatchRegistry::handle_call");
        match msg {
            PatchRegistryQuery::GetPatch(path) => {
                let path = self.resolve_index_path(path);
                let patch = self
                    .patches
                    .get(path.as_ref())
                    .map(|entry| entry.0.clone())
                    .filter(|html| !html.is_empty());
                response_sender
                    .send(PatchRegistryResponse::Patch(patch))
                    .drop_result();
            }
            PatchRegistryQuery::HasSource(path) => {
                response_sender
                    .send(PatchRegistryResponse::HasSource(
                        self.source_paths.contains(&path),
                    ))
                    .drop_result();
            }
            PatchRegistryQuery::Watch(path) => {
                let path = self.resolve_index_path(path).into_owned();
                let watch_receiver = match self.patches.entry(path) {
                    Entry::Occupied(entry) => entry.get().1.subscribe(),
                    Entry::Vacant(entry) => {
                        let (sender, receiver) = watch::channel(Default::default());
                        entry.insert((Default::default(), sender));
                        receiver
                    }
                };
                response_sender
                    .send(PatchRegistryResponse::WatchReceiver(watch_receiver))
                    .drop_result();
            }
        }
        Ok(())
    }
}

/// A request to modify the patch registry.
#[derive(Debug)]
pub enum PatchRegistryRequest {
    /// Register a new patch with rendered HTML content.
    NewPatch(PathBuf, String),
    /// The book is rebuilt, with an optional new index path.
    Rebuild {
        index_path: Option<PathBuf>,
        source_paths: HashSet<PathBuf>,
    },
    /// Clear the registry, like a soft shutdown.
    Clear,
}

/// A query for the patch registry.
#[derive(Debug)]
pub enum PatchRegistryQuery {
    GetPatch(PathBuf),
    HasSource(PathBuf),
    /// Watch a path for changes.
    Watch(PathBuf),
}

/// A response from patch registry.
#[derive(Debug)]
pub enum PatchRegistryResponse {
    Patch(Option<String>),
    HasSource(bool),
    /// Receiver to watch for patches.
    WatchReceiver(watch::Receiver<String>),
}

impl PatchRegistry {
    /// Convert HTTP `path` to the index path if it is the path to root.
    fn resolve_index_path(&self, path: PathBuf) -> Cow<'_, Path> {
        match &self.index_path {
            Some(index_path) if path == PathBuf::new() || path == Path::new("index.html") => {
                Cow::Borrowed(index_path)
            }
            _ => Cow::Owned(path),
        }
    }
}
