use super::*;

#[derive(Default)]
pub struct HtmlHbsState {
    pub path2ctxs: HashMap<PathBuf, CtxCore>,
    pub smart_punctuation: bool,
    /// Relative path of the source file of the index chapter.
    pub index_path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct CtxCore {
    pub chapter_name: String,
    pub len_content: usize,
}

pub const RENDERER: HtmlHandlebars = HtmlHandlebars {};

// NOTE: Below is adapted from
// <https://github.com/rust-lang/mdBook/blob/3bdcc0a5a6f3c85dd751350774261dbc357b02bd/src/renderer/html_handlebars/hbs_renderer.rs>.

pub fn html_config_n_theme_dir_n_theme_n_handlebars(
    ctx: &RenderContext,
) -> Result<(HtmlConfig, PathBuf, Theme, Handlebars<'static>)> {
    let html_config = {
        let mut h = ctx.config.html_config().unwrap_or_default();
        // NOTE: Inject the JavaScript for live patching.
        h.additional_js.push(LIVE_PATCH_PATH.into());
        h
    };

    let theme_dir = match html_config.theme {
        Some(ref theme) => {
            let dir = ctx.root.join(theme);
            if !dir.is_dir() {
                bail!("theme dir {} does not exist", dir.display());
            }
            dir
        }
        None => ctx.root.join("theme"),
    };
    let theme = Theme::new(theme_dir.clone());

    let mut handlebars = Handlebars::new();

    debug!("Register the index handlebars template");
    handlebars.register_template_string("index", String::from_utf8(theme.index.clone())?)?;

    debug!("Register the head handlebars template");
    handlebars.register_partial("head", String::from_utf8(theme.head.clone())?)?;

    debug!("Register the redirect handlebars template");
    handlebars.register_template_string("redirect", String::from_utf8(theme.redirect.clone())?)?;

    debug!("Register the header handlebars template");
    handlebars.register_partial("header", String::from_utf8(theme.header.clone())?)?;

    debug!("Register handlebars helpers");
    RENDERER.register_hbs_helpers(&mut handlebars, &html_config);

    Ok((html_config, theme_dir, theme, handlebars))
}

impl HtmlHbsState {
    /// Render the book to HTML using the Handlebars renderer and
    /// save intermediate state.
    pub async fn full_render(
        &mut self,
        ctx: &RenderContext,
        html_config: HtmlConfig,
        theme: &Theme,
        handlebars: &Handlebars<'_>,
    ) -> Result<()> {
        info!("Running the html backend for a full render.");
        let book_config = &ctx.config.book;
        let src_dir = ctx.root.join(&ctx.config.book.src);
        let destination = &ctx.destination;
        let book = &ctx.book;
        yield_now().await;

        if destination.exists() {
            utils::fs::remove_dir_content(destination)
                .with_context(|| "Unable to remove stale HTML output")?;
            yield_now().await;
        }

        trace!("render");
        let mut data = make_data(&ctx.root, book, &ctx.config, &html_config, theme)?;
        yield_now().await;

        // Print version
        let mut print_content = String::new();

        fs::create_dir_all(destination)
            .await
            .with_context(|| "Unexpected error when constructing destination path")?;

        self.smart_punctuation = html_config.smart_punctuation();
        let mut is_index = true;
        self.path2ctxs.clear();
        let items = || {
            book.iter().filter_map(|item| {
                if let BookItem::Chapter(Chapter {
                    name,
                    content,
                    source_path: Some(source_path),
                    ..
                }) = item
                {
                    Some((item, name, content, source_path))
                } else {
                    None
                }
            })
        };
        self.path2ctxs.reserve(items().count());

        for (item, name, content, source_path) in items() {
            // NOTE: We know that `HtmlHandlebars::render_item` only
            // renders non-draft chapters,
            // so we skip all other book items.
            let source_path = src_dir.join(source_path);
            if is_index {
                self.index_path = Some(source_path.strip_prefix(&src_dir)?.to_owned());
            }
            let ctx = RenderItemContext {
                handlebars,
                destination: destination.to_path_buf(),
                data: data.clone(),
                is_index,
                book_config: book_config.clone(),
                html_config: html_config.clone(),
                edition: ctx.config.rust.edition,
                chapter_titles: &ctx.chapter_titles,
            };
            // Only the first non-draft chapter item should be treated as the "index"
            is_index = false;
            block_n_yield(|| RENDERER.render_item(item, ctx, &mut print_content)).await?;
            let ctx = CtxCore {
                chapter_name: name.clone(),
                len_content: content.len(),
            };
            self.path2ctxs.insert(source_path, ctx);
        }

        // Render 404 page
        if html_config.input_404 != Some("".to_string()) {
            block_n_yield(|| {
                RENDERER.render_404(ctx, &html_config, &src_dir, handlebars, &mut data)
            })
            .await?;
        }

        // Print version
        block_n_yield(|| RENDERER.configure_print_version(&mut data, &print_content)).await;
        if let Some(ref title) = ctx.config.book.title {
            data.insert("title".to_owned(), json!(title));
        }

        // Render the handlebars template with the data
        if html_config.print.enable {
            debug!("Render template");
            let rendered = handlebars.render("index", &data)?;
            yield_now().await;

            let rendered = block_n_yield(|| {
                RENDERER.post_process(
                    rendered,
                    &html_config.playground,
                    &html_config.code,
                    ctx.config.rust.edition,
                )
            })
            .await;

            block_n_yield(|| utils::fs::write_file(destination, "print.html", rendered.as_bytes()))
                .await?;
            debug!("Created print.html ✓");
        }

        // Render search index
        let search = html_config.search.unwrap_or_default();
        if search.enable {
            debug!("Search indexing");
            block_n_yield(|| search::create_files(&search, destination, book)).await?;
        }

        debug!("Emitting redirects");
        block_n_yield(|| {
            RENDERER.emit_redirects(&ctx.destination, handlebars, &html_config.redirect)
        })
        .await
        .context("Unable to emit redirects")?;

        Ok(())
    }

    /// Patch the built book for the `paths` changed.
    ///
    /// - `paths` are absolute paths.
    ///
    /// # Limitation
    /// Each patched chapter is preprocessed and rendered individually without
    /// any context of other chapters in the book,
    /// so preprocessors that operate across multiple book items are
    /// not supported.
    pub async fn patch<'i, I: IntoIterator<Item = &'i PathBuf>>(
        &self,
        book: &MDBookCore,
        src_dir: &Path,
        paths: I,
        patch_registry_ref: &mut ActorRef<PatchRegistry>,
    ) -> Result<()> {
        for path in paths.into_iter() {
            let Some(CtxCore {
                chapter_name,
                len_content,
            }) = self.path2ctxs.get(path)
            else {
                continue;
            };
            let relative_path = path.strip_prefix(src_dir)?;
            debug!(?path, chapter_name, len_content, ?relative_path, "patching");

            let content = load_content_of_chapter(path, len_content * 2).await?;
            patch_chapter_w_content(
                chapter_name,
                content,
                relative_path,
                book,
                patch_registry_ref,
            )
            .await?;
        }
        Ok(())
    }
}

pub async fn patch_chapter_w_content(
    chapter_name: &str,
    content: String,
    relative_path: &Path,
    book: &MDBookCore,
    patch_registry_ref: &mut ActorRef<PatchRegistry>,
) -> Result<()> {
    // TODO: Spawn tasks into `patch_join_sets` instead of blocking.
    let chapter = Chapter::new(chapter_name, content, relative_path, vec![]);
    let mut patcher_book = Book::new();
    patcher_book.sections = vec![BookItem::Chapter(chapter)];
    let (mut preprocessed_book, _) = book.preprocess_book(patcher_book).await?;
    let markdown = match preprocessed_book.sections.pop() {
        None => bail!("{chapter_name} at {relative_path:?} preprocessed to an empty book."),
        Some(BookItem::Chapter(Chapter {
            content,
            source_path: Some(source_path),
            ..
        })) if source_path == relative_path => content,
        _ => bail!(
            "{chapter_name} at {relative_path:?} preprocessed to unexpected {preprocessed_book:?}"
        ),
    };
    patch_registry_ref
        .cast(PatchRegistryRequest::NewPatch(
            relative_path.with_extension("html"),
            markdown,
        ))
        .await
        .context("Updating the patch registry")?;
    Ok(())
}

async fn load_content_of_chapter(path: &PathBuf, capacity: usize) -> io::Result<String> {
    let mut content = String::with_capacity(capacity);
    {
        let mut f = File::open(path).await?;
        f.read_to_string(&mut content).await?;
    }
    if content.as_bytes().starts_with(b"\xef\xbb\xbf") {
        content.replace_range(..3, "");
    }
    content.shrink_to_fit();

    Ok(content)
}

/// Core fields of [MDBook] for separate rendering.
// NOTE: This is adapted from `MDBook`.
#[derive(Default)]
pub struct MDBookCore {
    /// The book's root directory.
    pub root: PathBuf,
    /// The configuration used to tweak now a book is built.
    pub config: Config,
    /// List of renderers to render the book.
    pub renderers: Vec<Box<dyn Renderer + Send + Sync + 'static>>,
    /// List of pre-processors to be run on the book.
    pub preprocessors: Vec<Box<dyn Preprocessor + Send + Sync + 'static>>,
}

impl MDBookCore {
    /// Run preprocessors on `book` and return the final book.
    pub async fn preprocess_book(&self, book: Book) -> Result<(Book, PreprocessorContext)> {
        let preprocess_ctx = PreprocessorContext {
            root: self.root.clone(),
            config: self.config.clone(),
            renderer: RENDERER.name().to_string(),
            mdbook_version: MDBOOK_VERSION.to_string(),
            chapter_titles: RefCell::new(HashMap::new()),
            __non_exhaustive: (),
        };
        // NOTE: This `Mutex` is needed because `PreprocessorContext: !Send`.
        let preprocess_ctx = Mutex::new(preprocess_ctx);
        let mut preprocessed_book = book;
        for preprocessor in &self.preprocessors {
            let should_run = || preprocessor_should_run(&**preprocessor, &RENDERER, &self.config);
            if block_n_yield(should_run).await {
                debug!(preprocessor = preprocessor.name(), "Running.",);
                let run = || preprocessor.run(&preprocess_ctx.lock().unwrap(), preprocessed_book);
                preprocessed_book = block_n_yield(run).await?;
            }
        }
        Ok((preprocessed_book, preprocess_ctx.into_inner().unwrap()))
    }
}
impl From<MDBook> for MDBookCore {
    fn from(value: MDBook) -> Self {
        Self {
            root: value.root,
            config: value.config,
            renderers: value.renderers,
            preprocessors: value.preprocessors,
        }
    }
}
