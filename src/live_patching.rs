use std::net::SocketAddr;

use mdbook_incremental_preview::live_patch_continuously;

use super::*;

#[derive(Debug)]
pub struct LivePatcher {
    book_root: PathBuf,
    patch_join_set: JoinSet<Result<()>>,
    preview_options: (SocketAddr, bool),
}

impl Actor for LivePatcher {
    type L = ();
    type T = LivePatcherInfo;
    type R = ();

    async fn handle_cast(&mut self, msg: Self::T, env: &mut ActorRef<Self>) -> Result<()> {
        debug!(?msg, "Handling cast.");
        match msg {
            LivePatcherInfo::BookRoot(book_root) if book_root == self.book_root => {
                debug!(?book_root, "Ignoring unchanged.");
            }
            LivePatcherInfo::BookRoot(book_root) => {
                debug!(?book_root, "Updating.");
                self.book_root = book_root;
                if !self.patch_join_set.is_empty() {
                    info!("Restarting live patching.");
                    let env = env.clone();
                    spawn(async move {
                        env.cast(LivePatcherInfo::StopPreview).await.drop_result();
                        let open_msg = LivePatcherInfo::OpenPreview(None);
                        env.cast(open_msg).await.drop_result();
                    });
                }
            }
            LivePatcherInfo::OpenPreview(maybe_options) => {
                if let Some(options) = maybe_options {
                    self.preview_options = options;
                }
                let (socket_address, open_browser) = self.preview_options;
                if self.patch_join_set.is_empty() {
                    info!(?self.preview_options, "Starting live patching.");
                    self.patch_join_set.spawn(live_patch_continuously(
                        self.book_root.clone(),
                        socket_address,
                        open_browser,
                    ));
                } else {
                    // TODO: Open the browser if requested.
                }
            }
            LivePatcherInfo::StopPreview => self.patch_join_set.shutdown().await,
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum LivePatcherInfo {
    /// Update the book root.
    BookRoot(PathBuf),
    /// Open preview at the socket address and if open the browser.
    OpenPreview(Option<(SocketAddr, bool)>),
    /// Stop the preview server.
    StopPreview,
}

impl Default for LivePatcher {
    fn default() -> Self {
        Self {
            book_root: ".".into(),
            patch_join_set: Default::default(),
            preview_options: (([127, 0, 0, 1], 3000).into(), true),
        }
    }
}
