use super::*;

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
}

impl Actor for Rebuilder {
    type L = ();
    type T = RebuildInfo;
    type R = ();

    async fn init(&mut self, env: &mut ActorRef<Self>) -> Result<()> {
        // Start with a full reload.
        env.cast(RebuildInfo::Rebuild(true)).await.drop_result();
        Ok(())
    }

    async fn handle_cast(&mut self, msg: Self::T, env: &mut ActorRef<Self>) -> Result<()> {
        match msg {
            RebuildInfo::Rebuild(reload) => {
                info!(?self.build_dir, "Full rebuild.");
                _ = self.mutables.rebuild_join_set.spawn(load_book(
                    self.book_root.clone(),
                    self.build_dir.clone(),
                    reload,
                    env.clone(),
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
                        process_cfg: hbs_state.process_cfg.clone(),
                    })
                    .await
                    .context("Clearing the patch registry")?;
                if reload {
                    self.handle_reload(&book, &html_config, &theme_dir, env)
                        .await?;
                }
                let m = &mut self.mutables;
                (m.book, m.html_config, m.theme_dir, m.hbs_state) =
                    (book.into(), html_config, theme_dir, hbs_state);
                // Re-patch the chapters patched after a rebuild.
                let paths = running_patch_join_sets(&mut m.patch_join_sets);
                let (env, msg) = (env.clone(), RebuildInfo::ChangedPaths(paths));
                spawn(async move { env.cast(msg).await.drop_result() });
            }
            RebuildInfo::ChangedPaths(paths) => {
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
                    Some(reload) => self.send_rebuild_info(env.clone(), reload),
                    None => {
                        let (b, ref_, sets) =
                            (&m.book, &self.patch_registry_ref, &mut m.patch_join_sets);
                        m.hbs_state.patch(b, &self.src_dir, paths, ref_, sets).await;
                    }
                }
            }
            RebuildInfo::ModifiedContent { path, content } => {
                let m = &mut self.mutables;
                if let Some((arc_path, ctx)) = m.hbs_state.path2ctxs.get_key_value(path.as_path()) {
                    let task = patch_chapter_w_content(
                        arc_path.clone(),
                        self.src_dir.clone(),
                        ctx.chapter_name.clone(),
                        content,
                        m.book.clone(),
                        self.patch_registry_ref.clone(),
                    );
                    _ = m.patch_join_sets.entry(path).or_default().spawn(task);
                }
            }
            RebuildInfo::OpenBrowser(path) => {
                self.mutables.open_browser_at = Some(path);
                self.maybe_open_browser();
            }
        }
        Ok(())
    }
}

pub enum RebuildInfo {
    /// Instruction to rebuild, and if a full reload should be considered.
    Rebuild(bool),
    /// Newly built book and state.
    NewBook(Box<BookData>),
    /// Paths changed.
    ChangedPaths(Vec<PathBuf>),
    /// Content of a modified path.
    ModifiedContent { path: PathBuf, content: String },
    /// Open the browser for the chapter of the given absolute path.
    OpenBrowser(PathBuf),
}

impl Rebuilder {
    async fn handle_reload(
        &mut self,
        book: &MDBookCore,
        html_config: &HtmlConfig,
        theme_dir: &Path,
        env: &ActorRef<Self>,
    ) -> Result<()> {
        let m = &mut self.mutables;
        let src_dir = book.root.join(&book.config.book.src);
        let src_dir_changed = src_dir != *self.src_dir;
        let theme_dir_changed = m.theme_dir != *theme_dir;
        let extra_watch_dirs_changed =
            m.book.config.build.extra_watch_dirs != book.config.build.extra_watch_dirs;

        let file_404_changed =
            m.book.config.get("output.html.input-404") != book.config.get("output.html.input-404");
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
            info!(
                ?self.book_root,
                ?src_dir,
                ?theme_dir,
                ?book.config.build.extra_watch_dirs,
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
                    &book.config.build.extra_watch_dirs,
                    event_handler,
                )
            };
            m._debouncer_to_keep_watcher_alive = Some(block_n_yield(watch).await);
        }

        if src_dir_changed || additional_js_changed || additional_css_changed || file_404_changed {
            let input_404 = book
                .config
                .get("output.html.input-404")
                .and_then(toml::Value::as_str)
                .unwrap_or("404.html");
            let relative_404_path = Path::new(input_404).with_extension("html");
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
                    theme_dir: theme_dir.into(),
                    additional_js: html_config.additional_js.clone(),
                    additional_css: html_config.additional_css.clone(),
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

    fn maybe_open_browser(&mut self) {
        let m = &mut self.mutables;
        if m.summary_md != PathBuf::default() {
            // We have done at least one rebuild.
            if let Some(path) = mem::take(&mut m.open_browser_at) {
                let path = path
                    .strip_prefix(&self.src_dir)
                    .unwrap_or(&path)
                    .with_extension("html");
                let address = format!("http://{}/{}", self.socket_address, path.display());
                spawn_blocking(move || open(address));
            }
        }
    }

    pub fn new(
        book_root: Arc<Path>,
        build_dir: Arc<Path>,
        socket_address: SocketAddr,
        info_tx: mpsc::Sender<ServeInfo>,
        patch_registry_ref: ActorRef<PatchRegistry>,
        open_browser_at: Option<PathBuf>,
        ignored_paths: IgnoredPaths,
    ) -> Self {
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
                open_browser_at,
                ignored_paths,
                ..Default::default()
            },
        }
    }
}

pub async fn patch_chapter_w_content(
    path: Arc<Path>,
    src_dir: Arc<Path>,
    chapter_name: Arc<str>,
    content: String,
    book: Arc<MDBookCore>,
    patch_registry_ref: ActorRef<PatchRegistry>,
) {
    let task = try_patch_chapter_w_content(
        &path,
        &src_dir,
        &chapter_name,
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
    if let Err(err) = try_load_book(&book_root, &build_dir, reload, Default::default(), env).await {
        error!(?err, "loading and preprocessing the book.");
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
    let (html_config, theme_dir, theme, handlebars) =
        block_n_yield(|| html_config_n_theme_dir_n_theme_n_handlebars(&render_context)).await?;
    hbs_state
        .full_render(render_context, html_config.clone(), &theme, &handlebars)
        .await?;
    info!(
        ?theme_dir,
        len_rendering_path2ctxs = hbs_state.path2ctxs.len(),
        ?hbs_state.index_path,
        "rebuilt the book"
    );
    env.cast(RebuildInfo::NewBook(Box::new(BookData {
        book: book.into(),
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
    open_browser_at: Option<PathBuf>,
    _debouncer_to_keep_watcher_alive: Option<Debouncer<RecommendedWatcher>>,
    book: Arc<MDBookCore>,
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
