use super::*;
use crate::source_search::encode_url_path;

// NOTE: Below is adapted from
// <https://github.com/rust-lang/mdBook/blob/3bdcc0a5a6f3c85dd751350774261dbc357b02bd/src/cmd/watch/native.rs>.

pub struct Rebuilder {
    book_root: Arc<Path>,
    build_dir: Arc<Path>,
    socket_address: SocketAddr,
    info_tx: mpsc::Sender<ServeInfo>,
    patch_registry_ref: ActorRef<PatchRegistry>,
    book_toml: PathBuf,
    src_dir: Arc<Path>,
    mutables: RebuilderMut,
    forward_search: broadcast::Sender<String>,
}

impl Actor for Rebuilder {
    type Call = ();
    type Cast = RebuildInfo;
    type Reply = ();

    async fn init(&mut self, env: &mut ActorEnv<Self>) -> Result<()> {
        // Start with a full reload.
        env.ref_
            .cast(RebuildInfo::Rebuild(true))
            .await
            .drop_result();
        Ok(())
    }

    async fn handle_cast(&mut self, msg: Self::Cast, env: &mut ActorEnv<Self>) -> Result<()> {
        match msg {
            RebuildInfo::Rebuild(reload) => {
                if self.mutables.rebuilding {
                    self.mutables.pending_rebuild =
                        Some(self.mutables.pending_rebuild.unwrap_or(false) || reload);
                    return Ok(());
                }
                self.mutables.rebuilding = true;
                info!(?self.build_dir, "Full rebuild.");
                _ = self.mutables.rebuild_join_set.spawn(load_book(
                    self.book_root.clone(),
                    self.build_dir.clone(),
                    reload,
                    env.ref_.clone(),
                ));
            }
            RebuildInfo::NewBook(data) => {
                let BookData {
                    book,
                    reload,
                    html_config,
                    theme_dir,
                    hbs_state,
                } = *data;
                self.patch_registry_ref
                    .cast(PatchRegistryRequest::Rebuild {
                        index_path: hbs_state.index_path.clone(),
                        source_paths: hbs_state
                            .path2ctxs
                            .keys()
                            .filter_map(|path| std::fs::canonicalize(path).ok())
                            .collect(),
                    })
                    .await
                    .context("Clearing the patch registry")?;
                if reload {
                    self.handle_reload(&book, &html_config, &theme_dir, &env.ref_)
                        .await?;
                }
                let book = Arc::new(book);
                let m = &mut self.mutables;
                (m.book, m.html_config, m.theme_dir, m.hbs_state) =
                    (Some(book), html_config, theme_dir, hbs_state);
                // Re-patch the chapters patched after a rebuild.
                let paths = running_patch_join_sets(&mut m.patch_join_sets)
                    .into_iter()
                    .filter(|path| !m.contents.contains_key(path))
                    .collect();
                let (ref_, msg) = (env.ref_.clone(), RebuildInfo::ChangedPaths(paths));
                spawn(async move { ref_.cast(msg).await.drop_result() });
                for (path, content) in self.mutables.contents.clone() {
                    self.patch_content(path, content);
                }
                self.maybe_forward_search();
                self.finish_rebuild(env.ref_.clone());
            }
            RebuildInfo::RebuildFailed => self.finish_rebuild(env.ref_.clone()),
            RebuildInfo::ChangedPaths(paths) => {
                // notify's access events also reach the mini debouncer; only changed
                // metadata should trigger rebuilds, otherwise reads cause a loop.
                let paths: Vec<_> = paths
                    .into_iter()
                    .filter(|path| {
                        let stamp = std::fs::metadata(path)
                            .ok()
                            .map(|metadata| (metadata.modified().ok(), metadata.len()));
                        self.mutables.file_stamps.insert(path.clone(), stamp) != Some(stamp)
                    })
                    .collect();
                if paths.is_empty() {
                    return Ok(());
                }
                info!(?paths, "Directories changed.");
                let m = &mut self.mutables;
                let full_rebuild = match &m.maybe_gitignore {
                    Some((_, gitignore_path)) if paths.contains(gitignore_path) => {
                        // Gitignore file changed,
                        // update the gitignore and make a full rebuild.
                        m.maybe_gitignore =
                            block_n_yield(|| maybe_make_gitignore(&self.book_root)).await;
                        debug!("Reloaded gitignore.");
                        Some(false)
                    }
                    // `book.toml` changed, make a full rebuild,
                    // reload the watcher and the server.
                    _ if paths.contains(&self.book_toml) => Some(true),
                    // `Summary.md` or theme changed, make a full rebuild.
                    _ if paths.contains(&m.summary_md)
                        || paths.iter().any(|path| path.starts_with(&m.theme_dir)) =>
                    {
                        Some(false)
                    }
                    _ => None,
                };
                debug!(full_rebuild);
                match full_rebuild {
                    Some(reload) => self.send_rebuild_info(env.ref_.clone(), reload),
                    None => {
                        if let Some(book) = &m.book {
                            let (ref_, sets) = (&self.patch_registry_ref, &mut m.patch_join_sets);
                            m.hbs_state
                                .patch(book, &self.src_dir, paths, ref_, sets)
                                .await;
                        }
                    }
                }
            }
            RebuildInfo::ModifiedContent { path, content } => {
                self.mutables.contents.insert(path.clone(), content.clone());
                self.patch_content(path, content);
            }
            RebuildInfo::Closed(path) => {
                let m = &mut self.mutables;
                m.contents.remove(&path);
                if let Some(mut tasks) = m.patch_join_sets.remove(&path) {
                    tasks.abort_all();
                }
                if let Some(book) = &m.book {
                    m.hbs_state
                        .patch(
                            book,
                            &self.src_dir,
                            [path],
                            &self.patch_registry_ref,
                            &mut m.patch_join_sets,
                        )
                        .await;
                }
            }
            RebuildInfo::OpenBrowser(path) => {
                self.mutables.open_browser_at = Some(path);
                self.maybe_open_browser();
            }
            RebuildInfo::ForwardSearch(location) => {
                self.mutables.pending_search = Some(location);
                self.maybe_forward_search();
            }
        }
        Ok(())
    }
}

pub enum RebuildInfo {
    RebuildFailed,
    Closed(PathBuf),
    ForwardSearch(SearchLocation),
    /// Instruction to rebuild, and if a full reload should be considered.
    Rebuild(bool),
    /// Newly built book and state.
    NewBook(Box<BookData>),
    /// Paths changed.
    ChangedPaths(Vec<PathBuf>),
    /// Content of a modified path.
    ModifiedContent {
        path: PathBuf,
        content: String,
    },
    /// Open the browser for the chapter of the given absolute path.
    OpenBrowser(PathBuf),
}

impl Rebuilder {
    fn patch_content(&mut self, path: PathBuf, content: String) {
        let m = &mut self.mutables;
        if let (Some(book), Some((arc_path, ctx))) =
            (&m.book, m.hbs_state.path2ctxs.get_key_value(path.as_path()))
        {
            let task = patch_chapter_w_content(
                arc_path.clone(),
                self.src_dir.clone(),
                ctx.chapter_name.clone(),
                ctx.html_path.clone(),
                content,
                book.clone(),
                self.patch_registry_ref.clone(),
            );
            let tasks = m.patch_join_sets.entry(path).or_default();
            tasks.abort_all();
            _ = tasks.spawn(task);
        }
    }
    async fn handle_reload(
        &mut self,
        book: &MDBookCore,
        html_config: &HtmlConfig,
        theme_dir: &Path,
        env: &ActorRef<Self>,
    ) -> Result<()> {
        let m = &mut self.mutables;
        let config = book.config();
        let src_dir = book.source_dir().to_path_buf();
        let src_dir_changed = src_dir != *self.src_dir;
        let theme_dir_changed = m.theme_dir != *theme_dir;
        let old_config = m.book.as_ref().map(|book| book.config());
        let extra_watch_dirs_changed = match &old_config {
            Some(old_config) => old_config.build.extra_watch_dirs != config.build.extra_watch_dirs,
            None => true,
        };
        let file_404_changed = match &old_config {
            Some(old_config) => input_404(old_config) != input_404(config),
            None => true,
        };
        let additional_js_changed = m.html_config.additional_js != html_config.additional_js;
        let additional_css_changed = m.html_config.additional_css != html_config.additional_css;

        debug!(
            src_dir_changed,
            theme_dir_changed,
            extra_watch_dirs_changed,
            file_404_changed,
            additional_js_changed,
            additional_css_changed,
        );
        yield_now().await;

        if src_dir_changed || theme_dir_changed || extra_watch_dirs_changed {
            for path in [self.book_toml.clone(), src_dir.join("SUMMARY.md")] {
                let stamp = std::fs::metadata(&path)
                    .ok()
                    .map(|metadata| (metadata.modified().ok(), metadata.len()));
                m.file_stamps.insert(path, stamp);
            }
            info!(
                ?self.book_root,
                ?src_dir,
                ?theme_dir,
                ?config.build.extra_watch_dirs,
                "Reloading the file watcher.",
            );
            let (env, ignored_paths) = (env.clone(), m.ignored_paths.clone());
            let event_handler = move |events: Result<Vec<DebouncedEvent>, _>| match events {
                Ok(events) if !events.is_empty() => {
                    let paths = events.into_iter().map(|event| event.path);
                    let paths = {
                        let ignored_paths = ignored_paths.read().unwrap();
                        paths.filter(|path| !ignored_paths.contains(path)).collect()
                    };
                    env.blocking_cast(RebuildInfo::ChangedPaths(paths))
                        .drop_result();
                }
                Ok(_) => {}
                Err(err) => error!(?err, "Watching for changes"),
            };
            let watch = || {
                watch_file_changes(
                    &self.book_root,
                    &src_dir,
                    theme_dir,
                    &self.book_toml,
                    &config.build.extra_watch_dirs,
                    event_handler,
                )
            };
            m._debouncer_to_keep_watcher_alive = Some(block_n_yield(watch).await);
        }

        if src_dir_changed || additional_js_changed || additional_css_changed || file_404_changed {
            let input_404 = html_config.get_404_output_file();
            let relative_404_path = Path::new(&input_404).with_extension("html");
            let file_404 = self.build_dir.join(relative_404_path);
            info!(
                ?src_dir,
                ?html_config.additional_js,
                ?html_config.additional_css,
                ?file_404,
                "Reloading the web server.",
            );
            self.info_tx
                .send(ServeInfo {
                    src_dir: src_dir.clone(),
                    file_404: file_404.clone(),
                })
                .await
                .context("The web server is unavailable to receive info.")?;
        }

        if src_dir_changed {
            (m.summary_md, self.src_dir) = (src_dir.join("SUMMARY.md"), src_dir.into());
        }
        self.maybe_open_browser();
        Ok(())
    }

    fn send_rebuild_info(&mut self, env: ActorRef<Self>, reload: bool) {
        spawn(async move {
            env.cast(RebuildInfo::Rebuild(reload)).await.drop_result();
        });
    }

    fn finish_rebuild(&mut self, env: ActorRef<Self>) {
        self.mutables.rebuilding = false;
        if let Some(reload) = self.mutables.pending_rebuild.take() {
            self.send_rebuild_info(env, reload);
        }
    }

    fn maybe_open_browser(&mut self) {
        let m = &mut self.mutables;
        if m.summary_md != PathBuf::default() {
            // We have done at least one rebuild.
            if let Some(path) = mem::take(&mut m.open_browser_at) {
                let path = path
                    .strip_prefix(&self.src_dir)
                    .unwrap_or(&path)
                    .with_extension("html");
                let address = format!("http://{}/{}", self.socket_address, encode_url_path(&path));
                spawn_blocking(move || open(address));
            }
        }
        self.maybe_forward_search();
    }

    fn maybe_forward_search(&mut self) {
        let Some(location) = self.mutables.pending_search.as_ref() else {
            return;
        };
        let Some(ctx) = self
            .mutables
            .hbs_state
            .path2ctxs
            .get(location.path.as_path())
        else {
            return;
        };
        let path = encode_url_path(&ctx.html_path);
        let url = format!("/{path}#mdbook-source-line={}", location.line);
        if self
            .forward_search
            .send(serde_json::json!({"url": url}).to_string())
            .is_err()
        {
            let address = format!("http://{}{url}", self.socket_address);
            spawn_blocking(move || open(address));
        }
        self.mutables.pending_search = None;
    }

    pub fn new(
        book_root: Arc<Path>,
        build_dir: Arc<Path>,
        socket_address: SocketAddr,
        info_tx: mpsc::Sender<ServeInfo>,
        patch_registry_ref: ActorRef<PatchRegistry>,
        open_browser_at: Option<PathBuf>,
        channels: (
            IgnoredPaths,
            broadcast::Sender<String>,
            HashMap<PathBuf, String>,
        ),
    ) -> Self {
        let (ignored_paths, forward_search, contents) = channels;
        let book_toml = book_root.join("book.toml");
        Self {
            book_root,
            build_dir,
            socket_address,
            info_tx,
            patch_registry_ref,
            book_toml,
            src_dir: Path::new("").into(),
            mutables: RebuilderMut {
                contents,
                open_browser_at,
                ignored_paths,
                ..Default::default()
            },
            forward_search,
        }
    }
}

fn input_404(config: &Config) -> Option<String> {
    config.html_config().and_then(|config| config.input_404)
}

pub async fn patch_chapter_w_content(
    path: Arc<Path>,
    src_dir: Arc<Path>,
    chapter_name: Arc<str>,
    html_path: PathBuf,
    content: String,
    book: Arc<MDBookCore>,
    patch_registry_ref: ActorRef<PatchRegistry>,
) {
    let task = try_patch_chapter_w_content(
        &path,
        &src_dir,
        &chapter_name,
        &html_path,
        content,
        &book,
        &patch_registry_ref,
    );

    if let Err(err) = task.await {
        error!(
            ?err,
            ?path,
            chapter_name = chapter_name.as_ref(),
            "Patching chapter with content.",
        );
    }
}

/// Paths of the chapters that are being patched.
fn running_patch_join_sets(patch_join_sets: &mut PatchJoinSets) -> Vec<PathBuf> {
    patch_join_sets
        .drain()
        .filter_map(|(path, mut join_set)| {
            _ = join_set.try_join_both();
            (!join_set.is_empty()).then_some(path)
        })
        .collect()
}

async fn load_book(
    book_root: Arc<Path>,
    build_dir: Arc<Path>,
    reload: bool,
    env: ActorRef<Rebuilder>,
) {
    if let Err(err) = try_load_book(
        &book_root,
        &build_dir,
        reload,
        Default::default(),
        env.clone(),
    )
    .await
    {
        error!(?err, "loading and preprocessing the book.");
        env.cast(RebuildInfo::RebuildFailed).await.drop_result();
    }
}

async fn try_load_book(
    book_root: &Path,
    build_dir: &Path,
    reload: bool,
    mut hbs_state: HtmlHbsState,
    env: ActorRef<Rebuilder>,
) -> Result<()> {
    let mut book = block_n_yield(|| MDBook::load(book_root)).await?;
    config_book_for_live_reload(&mut book).context("configuring the book for live reload")?;
    let render_context = block_n_yield(|| make_render_context(&book, build_dir)).await?;
    let (html_config, theme_dir) =
        block_n_yield(|| html_config_n_theme_dir(&render_context)).await?;
    hbs_state.full_render(render_context).await?;
    info!(
        ?theme_dir,
        len_rendering_path2ctxs = hbs_state.path2ctxs.len(),
        ?hbs_state.index_path,
        "rebuilt the book"
    );
    env.cast(RebuildInfo::NewBook(Box::new(BookData {
        book: MDBookCore::from(book),
        reload,
        html_config,
        theme_dir,
        hbs_state,
    })))
    .await
    .drop_result();
    Ok(())
}

pub struct BookData {
    pub book: MDBookCore,
    pub reload: bool,
    pub html_config: HtmlConfig,
    pub theme_dir: PathBuf,
    pub hbs_state: HtmlHbsState,
}

pub type PatchJoinSets = HashMap<PathBuf, TwoJoinSet<()>>;

/// The mutable parts of [`Rebuilder`].
#[derive(Default)]
pub struct RebuilderMut {
    rebuilding: bool,
    pending_rebuild: Option<bool>,
    file_stamps: HashMap<PathBuf, Option<(Option<std::time::SystemTime>, u64)>>,
    contents: HashMap<PathBuf, String>,
    pending_search: Option<SearchLocation>,
    open_browser_at: Option<PathBuf>,
    _debouncer_to_keep_watcher_alive: Option<Debouncer<RecommendedWatcher>>,
    book: Option<Arc<MDBookCore>>,
    maybe_gitignore: Option<(Gitignore, PathBuf)>,
    summary_md: PathBuf,
    theme_dir: PathBuf,
    html_config: HtmlConfig,
    hbs_state: HtmlHbsState,
    rebuild_join_set: TwoJoinSet<()>,
    /// [`TwoJoinSet`]s of each patched chapter's absolute path.
    patch_join_sets: PatchJoinSets,
    ignored_paths: IgnoredPaths,
}
