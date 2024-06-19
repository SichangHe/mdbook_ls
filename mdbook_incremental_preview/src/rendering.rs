use super::*;

pub struct StatefulHtmlHbs<'a> {
    pub path2ctxs: HashMap<PathBuf, (RenderItemContext<'a>, &'a Chapter)>,
}

pub const RENDERER: HtmlHandlebars = HtmlHandlebars {};

// NOTE: Below is adapted from
// <https://github.com/rust-lang/mdBook/blob/3bdcc0a5a6f3c85dd751350774261dbc357b02bd/src/renderer/html_handlebars/hbs_renderer.rs>.

pub fn html_config_n_theme_dir_n_theme_n_handlebars(
    ctx: &RenderContext,
) -> Result<(HtmlConfig, PathBuf, Theme, Handlebars)> {
    let html_config = ctx.config.html_config().unwrap_or_default();

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

impl<'a> StatefulHtmlHbs<'a> {
    /// Render the book to HTML using the Handlebars renderer and
    /// save intermediate state.
    pub fn render(
        ctx: &'a RenderContext,
        html_config: HtmlConfig,
        theme: &'a Theme,
        handlebars: &'a Handlebars<'a>,
    ) -> Result<Self> {
        info!("Running the html backend for a full render.");
        let book_config = &ctx.config.book;
        let src_dir = ctx.root.join(&ctx.config.book.src);
        let destination = &ctx.destination;
        let book = &ctx.book;
        let build_dir = ctx.root.join(&ctx.config.build.build_dir);

        if destination.exists() {
            utils::fs::remove_dir_content(destination)
                .with_context(|| "Unable to remove stale HTML output")?;
        }

        trace!("render");
        let mut data = make_data(&ctx.root, book, &ctx.config, &html_config, theme)?;

        // Print version
        let mut print_content = String::new();

        fs::create_dir_all(destination)
            .with_context(|| "Unexpected error when constructing destination path")?;

        let mut is_index = true;
        let path2ctxs = book
            .iter()
            .filter_map(|item| -> Option<Result<_>> {
                match item {
                    BookItem::Chapter(
                        chapter @ Chapter {
                            source_path: Some(source_path),
                            ..
                        },
                    ) => Some({
                        // NOTE: We know that `HtmlHandlebars::render_item` only
                        // renders non-draft chapters,
                        // so we skip all other book items.
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
                        RENDERER
                            .render_item(item, &mut ctx, &mut print_content)
                            .map(|_| (src_dir.join(source_path), (ctx, chapter)))
                    }),
                    _ => None,
                }
            })
            .collect::<Result<HashMap<_, _>>>()?;

        // Render 404 page
        if html_config.input_404 != Some("".to_string()) {
            RENDERER.render_404(ctx, &html_config, &src_dir, handlebars, &mut data)?;
        }

        // Print version
        RENDERER.configure_print_version(&mut data, &print_content);
        if let Some(ref title) = ctx.config.book.title {
            data.insert("title".to_owned(), json!(title));
        }

        // Render the handlebars template with the data
        if html_config.print.enable {
            debug!("Render template");
            let rendered = handlebars.render("index", &data)?;

            let rendered = RENDERER.post_process(
                rendered,
                &html_config.playground,
                &html_config.code,
                ctx.config.rust.edition,
            );

            utils::fs::write_file(destination, "print.html", rendered.as_bytes())?;
            debug!("Creating print.html ✓");
        }

        debug!("Copy static files");
        RENDERER
            .copy_static_files(destination, theme, &html_config)
            .with_context(|| "Unable to copy across static files")?;
        RENDERER
            .copy_additional_css_and_js(&html_config, &ctx.root, destination)
            .with_context(|| "Unable to copy across additional CSS and JS")?;

        // Render search index
        let search = html_config.search.unwrap_or_default();
        if search.enable {
            search::create_files(&search, destination, book)?;
        }

        RENDERER
            .emit_redirects(&ctx.destination, handlebars, &html_config.redirect)
            .context("Unable to emit redirects")?;

        // Copy all remaining files, avoid a recursive copy from/to the book build dir
        utils::fs::copy_files_except_ext(&src_dir, destination, true, Some(&build_dir), &["md"])?;

        Ok(Self { path2ctxs })
    }

    /// Patch the built book for the `paths` changed.
    ///
    /// # Limitation
    /// Each patched chapter is preprocessed and rendered individually without
    /// any context of other chapters in the book,
    /// so preprocessors that operate across multiple book items, like `link`,
    /// are not supported.
    pub fn patch<'i, I: IntoIterator<Item = &'i PathBuf>>(
        &mut self,
        book: &mut MDBook,
        paths: I,
    ) -> Result<()> {
        let original_book_preserved = mem::take(&mut book.book);

        // TODO: Support preprocessors like `link`.
        for path in paths.into_iter() {
            let Some((ctx, chapter)) = self.path2ctxs.get_mut(path) else {
                continue;
            };
            debug!(?path, ?chapter.name, ?ctx.is_index, "patching");

            let content = load_content_of_chapter(path, chapter)?;
            let chapter = Chapter::new(&chapter.name, content, path, vec![]);
            let mut patcher_book = Book::new();
            patcher_book.sections = vec![BookItem::Chapter(chapter)];
            book.book = patcher_book;
            let (preprocessed_book, _) = book.preprocess_book(&RENDERER)?;
            let item = preprocessed_book
                .iter()
                .next()
                .with_context(|| format!("{:?} preprocessed to an empty book.", book.book))?;
            RENDERER.render_item(item, ctx, &mut String::new())?;
        }

        book.book = original_book_preserved;
        Ok(())
    }
}

fn load_content_of_chapter(path: &PathBuf, chapter: &Chapter) -> io::Result<String> {
    let mut content = String::with_capacity(chapter.content.len() * 2);
    {
        let mut f = File::open(path)?;
        f.read_to_string(&mut content)?;
    }
    if content.as_bytes().starts_with(b"\xef\xbb\xbf") {
        content.replace_range(..3, "");
    }
    content.shrink_to_fit();

    Ok(content)
}
