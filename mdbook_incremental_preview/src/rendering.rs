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
) -> Result<(HtmlConfig, PathBuf, Theme, Handlebars)> {
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
            let mut ctx = RenderItemContext {
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
            block_in_place(|| RENDERER.render_item(item, &mut ctx, &mut print_content))?;
            let ctx = CtxCore {
                chapter_name: name.clone(),
                len_content: content.len(),
            };
            self.path2ctxs.insert(source_path, ctx);
        }

        // Render 404 page
        if html_config.input_404 != Some("".to_string()) {
            block_in_place(|| {
                RENDERER.render_404(ctx, &html_config, &src_dir, handlebars, &mut data)
            })?;
        }

        // Print version
        block_in_place(|| RENDERER.configure_print_version(&mut data, &print_content));
        if let Some(ref title) = ctx.config.book.title {
            data.insert("title".to_owned(), json!(title));
        }

        // Render the handlebars template with the data
        if html_config.print.enable {
            debug!("Render template");
            let rendered = handlebars.render("index", &data)?;
            yield_now().await;

            let rendered = block_in_place(|| {
                RENDERER.post_process(
                    rendered,
                    &html_config.playground,
                    &html_config.code,
                    ctx.config.rust.edition,
                )
            });

            block_in_place(|| {
                utils::fs::write_file(destination, "print.html", rendered.as_bytes())
            })?;
            debug!("Created print.html ✓");
        }

        // Render search index
        let search = html_config.search.unwrap_or_default();
        if search.enable {
            debug!("Search indexing");
            block_in_place(|| search::create_files(&search, destination, book))?;
        }

        debug!("Emitting redirects");
        block_in_place(|| {
            RENDERER.emit_redirects(&ctx.destination, handlebars, &html_config.redirect)
        })
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
        book: &mut MDBook,
        src_dir: &Path,
        paths: I,
        patch_registry_ref: &mut ActorRef<PatchRegistry>,
    ) -> Result<()> {
        let original_book_preserved = mem::take(&mut book.book);

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
            let chapter = Chapter::new(chapter_name, content, relative_path, vec![]);
            let mut patcher_book = Book::new();
            patcher_book.sections = vec![BookItem::Chapter(chapter)];
            book.book = patcher_book;
            let (preprocessed_book, _) = block_in_place(|| book.preprocess_book(&RENDERER))?;

            let markdown = match preprocessed_book.iter().next() {
                None => bail!("{:?} preprocessed to an empty book.", book.book),
                Some(BookItem::Chapter(Chapter {
                    content,
                    source_path: Some(source_path),
                    ..
                })) if source_path == relative_path => content,
                _ => bail!(
                    "{:?} preprocessed to unexpected {preprocessed_book:?}",
                    book.book
                ),
            };
            let html = block_in_place(|| utils::render_markdown(markdown, self.smart_punctuation));

            patch_registry_ref
                .cast(PatchRegistryRequest::NewPatch(
                    relative_path.with_extension("html"),
                    html,
                ))
                .await
                .context("Updating the patch registry")?;
        }

        // NOTE: Not restoring if an error occurs,
        // but would be fine because we would do a full rebuild anyway.
        book.book = original_book_preserved;
        Ok(())
    }
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
