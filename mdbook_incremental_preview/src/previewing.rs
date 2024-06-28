use super::*;

pub type IgnoredPaths = Arc<RwLock<HashSet<PathBuf>>>;

pub struct Previewer {
    build_temp_dir: TempDir,
    book_root: Arc<Path>,
    socket_address: SocketAddr,
    open_browser_at: Option<PathBuf>,
    versions: HashMap<PathBuf, i32>,
    ignored_paths: IgnoredPaths,
    patch_registry: Option<(
        ActorHandle<ActorMsg<PatchRegistry>>,
        ActorRef<PatchRegistry>,
    )>,
    rebuilder: Option<(ActorHandle<ActorMsg<Rebuilder>>, ActorRef<Rebuilder>)>,
    server: Option<JoinHandle<()>>,
}

impl Previewer {
    pub fn try_new() -> Result<Self> {
        Ok(Self {
            build_temp_dir: tempdir()?,
            book_root: Path::new("").into(),
            socket_address: ([127, 0, 0, 1], 3000).into(),
            open_browser_at: Some("".into()),
            versions: Default::default(),
            ignored_paths: Default::default(),
            patch_registry: None,
            rebuilder: None,
            server: None,
        })
    }

    /// This function does not check if the actors and
    /// tasks have already been started;
    /// the caller is responsible for stopping them.
    async fn start(&mut self, env: &ActorRef<Self>) {
        let (info_tx, info_rx) = mpsc::channel(8);
        info!(?self.socket_address, ?self.open_browser_at, "Starting live patching.");

        let rebuilder = Rebuilder::new(
            self.book_root.clone(),
            self.build_dir().into(),
            self.socket_address,
            info_tx.clone(),
            self.get_or_make_patch_registry(env),
            self.open_browser_at.take(),
            self.ignored_paths.clone(),
        );
        yield_now().await;
        let (handle, rebuilder_ref) =
            rebuilder.spawn_with_token(env.cancellation_token.child_token());
        self.rebuilder = Some((handle, rebuilder_ref.clone()));

        // TODO: Rid `serve_reloading` and combine the functionality into
        // `LivePatcher`.
        yield_now().await;
        self.server = Some(spawn(serve_reloading(
            self.book_root.to_path_buf(),
            self.socket_address,
            self.build_dir().to_owned(),
            rebuilder_ref,
            info_rx,
            self.get_or_make_patch_registry(env),
        )));
    }

    fn build_dir(&self) -> &Path {
        self.build_temp_dir.path()
    }

    async fn stop(&mut self) {
        self.maybe_stop_web_server();
        if let Some(handle) = mem::take(&mut self.server) {
            handle.await.drop_result();
        }
        if let Some((handle, actor_ref)) = mem::take(&mut self.rebuilder) {
            let msg = "shutting down the Rebuilder.";
            shut_down_actor_n_log_err::<Rebuilder>(handle, actor_ref, msg).await;
        }
        if let Some((_, actor_ref)) = &self.patch_registry {
            if let Err(err) = actor_ref.cast(PatchRegistryRequest::Clear).await {
                warn!(?err, "PatchRegistry died. Marking it dead.");
                let (handle, actor_ref) = mem::take(&mut self.patch_registry).unwrap();
                let msg = "shutting down the PatchRegistry.";
                shut_down_actor_n_log_err::<PatchRegistry>(handle, actor_ref, msg).await;
            }
        }
    }

    fn maybe_stop_web_server(&self) {
        if let Some(handle) = &self.server {
            debug!("Stopping web server.");
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

impl Actor for Previewer {
    type L = ();
    type T = PreviewInfo;
    type R = ();
    async fn handle_cast(&mut self, msg: Self::T, env: &mut ActorRef<Self>) -> Result<()> {
        match msg {
            PreviewInfo::BookRoot(book_root) if book_root == *self.book_root => {
                debug!(?book_root, "Ignoring unchanged.");
            }
            PreviewInfo::BookRoot(book_root) => {
                debug!(?book_root, "Updating.");
                self.book_root = book_root.into();
                if self.rebuilder.is_some() {
                    info!("Restarting live patching.");
                    self.stop().await;
                    self.start(env).await;
                }
            }
            PreviewInfo::OpenPreview {
                socket_address,
                open_browser_at,
            } => {
                debug!(?socket_address, ?open_browser_at, "Opening preview.");
                _ = socket_address.map(|v| self.socket_address = v);
                _ = open_browser_at.map(|v| self.open_browser_at = Some(v));
                match &self.rebuilder {
                    Some((_, ref_)) => {
                        info!("Already started live patching; not restarting.");
                        if let Some(open_browser_at) = mem::take(&mut self.open_browser_at) {
                            let msg = RebuildInfo::OpenBrowser(open_browser_at);
                            ref_.cast(msg).await.drop_result();
                        }
                    }
                    None => self.start(env).await,
                }
            }
            PreviewInfo::StopPreview => {
                info!("Stopping live patching.");
                self.stop().await;
            }
            PreviewInfo::Opened { path, version } => {
                debug!(?path, version, "Opened. Starting ignoring its file events.");
                self.ignored_paths.write().unwrap().insert(path.clone());
                self.versions
                    .entry(path)
                    .and_modify(|v| *v = version.max(*v))
                    .or_insert(version);
            }
            PreviewInfo::ModifiedContent {
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
                            debug!(?path, version, "Modified content.");
                            let msg = RebuildInfo::ModifiedContent { path, content };
                            rebuilder_ref.cast(msg).await.expect("Rebuilder died.");
                        }
                        None => debug!(?path, "Ignoring modified content, without rebuilder."),
                    }
                }
            }
            PreviewInfo::Closed(path) => {
                debug!(?path, "Closed. Stopping ignoring its file events.");
                self.versions.remove(&path);
                self.ignored_paths.write().unwrap().remove(&path);
            }
        }
        Ok(())
    }

    async fn before_exit(
        &mut self,
        run_result: Result<()>,
        _env: &mut ActorRef<Self>,
        _msg_receiver: &mut mpsc::Receiver<ActorMsg<Self>>,
    ) -> Result<()> {
        self.stop().await;
        if let Some((handle, actor_ref)) = mem::take(&mut self.patch_registry) {
            let msg = "shutting down the PatchRegistry.";
            shut_down_actor_n_log_err::<PatchRegistry>(handle, actor_ref, msg).await;
        }
        run_result
    }
}

#[derive(Clone, Debug)]
pub enum PreviewInfo {
    /// Update the book root.
    BookRoot(PathBuf),
    OpenPreview {
        socket_address: Option<SocketAddr>,
        /// Absolute path of the chapter file to open the browser at.
        open_browser_at: Option<PathBuf>,
    },
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

impl Drop for Previewer {
    fn drop(&mut self) {
        self.maybe_stop_web_server();
    }
}
