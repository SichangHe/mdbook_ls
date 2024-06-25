use super::*;

pub struct LivePatcher {
    build_temp_dir: TempDir,
    book_root: PathBuf,
    socket_address: SocketAddr,
    open_preview: bool,
    versions: HashMap<PathBuf, i32>,
    patch_registry: Option<(
        ActorHandle<ActorMsg<PatchRegistry>>,
        ActorRef<PatchRegistry>,
    )>,
    rebuilder: Option<(ActorHandle<ActorMsg<Rebuilder>>, ActorRef<Rebuilder>)>,
    server: Option<JoinHandle<()>>,
}

impl LivePatcher {
    pub fn try_new() -> Result<Self> {
        Ok(Self {
            build_temp_dir: tempdir()?,
            book_root: Default::default(),
            socket_address: ([127, 0, 0, 1], 3000).into(),
            open_preview: true,
            versions: Default::default(),
            patch_registry: None,
            rebuilder: None,
            server: None,
        })
    }

    fn start(&mut self, env: &ActorRef<Self>) {
        let serving_url = self.serving_url();
        let (info_tx, info_rx) = mpsc::channel(8);
        info!(?serving_url, "Starting live patching.");

        let rebuilder = Rebuilder::new(
            self.book_root.clone(),
            self.build_dir().to_owned(),
            info_tx.clone(),
            self.get_or_make_patch_registry(env),
            serving_url,
        );
        let (handle, rebuilder_ref) =
            rebuilder.spawn_with_token(env.cancellation_token.child_token());
        self.rebuilder = Some((handle, rebuilder_ref.clone()));

        // TODO: Rid `serve_reloading` and combine the functionality into
        // `LivePatcher`.
        self.server = Some(spawn(serve_reloading(
            self.book_root.clone(),
            self.socket_address,
            self.build_dir().to_owned(),
            rebuilder_ref,
            info_rx,
            self.get_or_make_patch_registry(env),
        )))
    }

    fn serving_url(&self) -> Option<String> {
        self.open_preview
            .then(|| format!("http://{}", self.socket_address))
    }

    fn build_dir(&self) -> &Path {
        self.build_temp_dir.path()
    }

    async fn stop(&mut self) {
        self.maybe_stop_web_server();
        if let Some((handle, actor_ref)) = mem::take(&mut self.rebuilder) {
            actor_ref.cancel();
            if let Err(err) = try_join_actor_handle(handle).await {
                error!(?err, "joininig the rebuilder's handle.");
            }
        }
        if let Some(handle) = mem::take(&mut self.server) {
            if let Err(err) = handle.await {
                error!(?err, "joininig the preview server's handle.");
            }
        }
    }

    fn maybe_stop_web_server(&self) {
        if let Some(handle) = &self.server {
            handle.abort();
        }
    }

    fn get_or_make_patch_registry(&mut self, env: &ActorRef<Self>) -> ActorRef<PatchRegistry> {
        if let Some((_, actor_ref)) = &self.patch_registry {
            actor_ref.clone()
        } else {
            let (handle, actor_ref) =
                PatchRegistry::default().spawn_with_token(env.cancellation_token.child_token());
            self.patch_registry = Some((handle, actor_ref.clone()));
            actor_ref
        }
    }
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
                if self.rebuilder.is_some() {
                    info!("Restarting live patching.");
                    self.stop().await;
                    self.start(env);
                }
            }
            LivePatcherInfo::OpenPreview(options) => {
                if let Some((socket_address, open_preview)) = options {
                    (self.socket_address, self.open_preview) = (socket_address, open_preview);
                }
                match self.rebuilder {
                    Some(_) => {
                        info!("Already started live patching; not restarting.");
                        if let Some(serving_url) = self.serving_url() {
                            spawn_blocking(|| open(serving_url));
                        }
                    }
                    None => self.start(env),
                }
            }
            LivePatcherInfo::StopPreview => {
                info!("Stopping live patching.");
                self.stop().await;
            }
            LivePatcherInfo::Opened { path, version } => {
                // TODO: Pause watching `path`.
                self.versions
                    .entry(path)
                    .and_modify(|v| *v = version.max(*v))
                    .or_insert(version);
            }
            LivePatcherInfo::ModifiedContent {
                path,
                version,
                content,
            } => {
                let updated = match self.versions.get_mut(&path) {
                    Some(v) if *v < version => {
                        *v = version;
                        true
                    }
                    Some(_) => {
                        debug!(?path, version, "Ignoring out-of-order modification update.");
                        false
                    }
                    None => {
                        self.versions.insert(path.clone(), version);
                        true
                    }
                };
                if updated {
                    match &self.rebuilder {
                        Some((_, rebuilder_ref)) => {
                            let msg = RebuildInfo::ModifiedContent { path, content };
                            rebuilder_ref.cast(msg).await.expect("Rebuilder died.");
                        }
                        None => debug!(?path, "Ignoring modified content, without rebuilder."),
                    }
                }
            }
            LivePatcherInfo::Closed(path) => {
                self.versions.remove(&path);
                // TODO: Resume watching `path`.
            }
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
    /// Opened path.
    Opened { path: PathBuf, version: i32 },
    /// Content of a modified path.
    ModifiedContent {
        path: PathBuf,
        version: i32,
        content: String,
    },
    /// Closed path.
    Closed(PathBuf),
}

impl Drop for LivePatcher {
    fn drop(&mut self) {
        self.maybe_stop_web_server();
    }
}
