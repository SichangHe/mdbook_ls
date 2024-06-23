use std::collections::hash_map::Entry;

use super::*;

/// A registry of watch channel senders of patches for paths.
#[derive(Default)]
pub struct PatchRegistry {
    patches: HashMap<PathBuf, watch::Sender<String>>,
}

impl Actor for PatchRegistry {
    type L = PatchRegistryQuery;
    type T = PatchRegistryRequest;
    type R = PatchRegistryResponse;

    async fn handle_cast(&mut self, msg: Self::T, _env: &mut ActorRef<Self>) -> Result<()> {
        match msg {
            PatchRegistryRequest::NewPatch(path, patch) => {
                debug!(?path, "Registry received patch.");
                match self.patches.entry(path) {
                    // Entry exists,
                    // update the patch in-place and send watch updates.
                    Entry::Occupied(mut entry) => {
                        let sender = entry.get_mut();
                        // Update the patch only if it changed.
                        if *sender.borrow() != patch {
                            debug!("Updating patch in registry.");
                            sender.send_modify(|old_patch| *old_patch = patch);
                        }
                    }
                    // New entry, register the patch and a new watch channel.
                    Entry::Vacant(entry) => _ = entry.insert(watch::channel(patch).0),
                };
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
        match msg {
            PatchRegistryQuery::Watch(path) => {
                let watch_receiver = match self.patches.entry(path) {
                    Entry::Occupied(entry) => entry.get().subscribe(),
                    Entry::Vacant(entry) => {
                        let (sender, receiver) = watch::channel(Default::default());
                        entry.insert(sender);
                        receiver
                    }
                };
                response_sender
                    .send(PatchRegistryResponse::WatchReceiver(watch_receiver))
                    .drop_result();
            }
            PatchRegistryQuery::GetHasPatch(path) => {
                let has_patch = self.patches.contains_key(&path);
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
    /// Register a new patch.
    NewPatch(PathBuf, String),
    /// Clear the registry.
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
