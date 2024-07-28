use std::collections::hash_map::Entry;

use super::*;

/// A registry of watch channel senders of patches for paths.
#[derive(Default)]
pub struct PatchRegistry {
    /// Preprocessed markdown content and watch channel for
    /// HTML `<main>` body content of each patched path.
    patches: HashMap<PathBuf, (String, watch::Sender<String>)>,
    /// Relative HTTP path of the index chapter.
    index_path: Option<PathBuf>,
    process_cfg: ProcessCfg,
}

impl Actor for PatchRegistry {
    type L = PatchRegistryQuery;
    type T = PatchRegistryRequest;
    type R = PatchRegistryResponse;

    async fn handle_cast(&mut self, msg: Self::T, _env: &mut ActorRef<Self>) -> Result<()> {
        match msg {
            PatchRegistryRequest::NewPatch(path, new_markdown) => {
                debug!(?path, "Registry received patch.");
                match self.patches.entry(path) {
                    // Entry exists,
                    // update the patch in-place and send watch updates.
                    Entry::Occupied(mut entry) => {
                        let (markdown, sender) = entry.get_mut();
                        // Update the patch only if it changed.
                        if *markdown != new_markdown {
                            debug!("Updating patch in registry.");
                            let rendered =
                                block_n_yield(|| self.process_cfg.render_markdown(&new_markdown))
                                    .await;
                            let new_html =
                                block_n_yield(|| self.process_cfg.post_process(rendered)).await;
                            sender.send_modify(|html| *html = new_html);
                        }
                    }
                    // New entry, register the patch and a new watch channel.
                    Entry::Vacant(entry) => {
                        _ = entry.insert((Default::default(), watch::channel(new_markdown).0))
                    }
                };
            }
            PatchRegistryRequest::Rebuild {
                index_path,
                process_cfg,
            } => {
                for (_, (_, watcher)) in self.patches.drain() {
                    watcher.send_modify(|v| *v = "__RELOAD".into())
                }
                self.process_cfg = process_cfg;
                if let Some(index_path) = index_path {
                    self.index_path = Some(index_path.with_extension("html"));
                    debug!(?self.index_path, ?self.process_cfg, "Updated index path in patch registry.")
                }
            }
            PatchRegistryRequest::Clear => self.patches.clear(),
        }
        Ok(())
    }

    async fn handle_call(
        &mut self,
        msg: Self::L,
        _env: &mut ActorRef<Self>,
        response_sender: oneshot::Sender<Self::R>,
    ) -> Result<()> {
        debug!(?msg, "PatchRegistry::handle_call");
        match msg {
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
            PatchRegistryQuery::GetHasPatch(path) => {
                let path = self.resolve_index_path(path);
                let has_patch = self.patches.contains_key(path.as_path());
                response_sender
                    .send(PatchRegistryResponse::HasPatch(has_patch))
                    .drop_result();
            }
        }
        Ok(())
    }
}

/// A request to modify the patch registry.
#[derive(Debug)]
pub enum PatchRegistryRequest {
    /// Register a new patch with the preprocessed Markdown content.
    NewPatch(PathBuf, String),
    /// The book is rebuilt, with an optional new index path.
    Rebuild {
        index_path: Option<PathBuf>,
        process_cfg: ProcessCfg,
    },
    /// Clear the registry, like a soft shutdown.
    Clear,
}

/// A query for the patch registry.
#[derive(Debug)]
pub enum PatchRegistryQuery {
    /// Watch a path for changes.
    Watch(PathBuf),
    /// Get if a path has patches.
    GetHasPatch(PathBuf),
}

/// A response from patch registry.
#[derive(Debug)]
pub enum PatchRegistryResponse {
    /// Receiver to watch for patches.
    WatchReceiver(watch::Receiver<String>),
    /// If a path has patches.
    HasPatch(bool),
}

impl PatchRegistry {
    /// Convert HTTP `path` to the index path if it is the path to root.
    fn resolve_index_path(&self, path: PathBuf) -> Cow<'_, PathBuf> {
        match &self.index_path {
            Some(index_path) if path == PathBuf::new() => Cow::Borrowed(index_path),
            _ => Cow::Owned(path),
        }
    }
}
