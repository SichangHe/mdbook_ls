use super::*;

pub struct StatefulHtmlHbs {}

impl StatefulHtmlHbs {
    /// Render the book to HTML using the Handlebars renderer and
    /// save intermediate state.
    pub fn render(&mut self, ctx: &RenderContext) -> Result<()> {
        // NOTE: Below is adapted from
        // <https://github.com/rust-lang/mdBook/blob/3bdcc0a5a6f3c85dd751350774261dbc357b02bd/src/renderer/html_handlebars/hbs_renderer.rs>.
        let book_config = &ctx.config.book;
        let html_config = ctx.config.html_config().unwrap_or_default();
        let src_dir = ctx.root.join(&ctx.config.book.src);
        let destination = &ctx.destination;
        let book = &ctx.book;
        let build_dir = ctx.root.join(&ctx.config.build.build_dir);

        if destination.exists() {
            utils::fs::remove_dir_content(destination)
                .with_context(|| "Unable to remove stale HTML output")?;
        }

        trace!("render");
        let mut handlebars = Handlebars::new();

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

        let theme = theme::Theme::new(theme_dir);

        debug!("Register the index handlebars template");
        handlebars.register_template_string("index", String::from_utf8(theme.index.clone())?)?;

        debug!("Register the head handlebars template");
        handlebars.register_partial("head", String::from_utf8(theme.head.clone())?)?;

        debug!("Register the redirect handlebars template");
        handlebars
            .register_template_string("redirect", String::from_utf8(theme.redirect.clone())?)?;

        debug!("Register the header handlebars template");
        handlebars.register_partial("header", String::from_utf8(theme.header.clone())?)?;

        let renderer = HtmlHandlebars {};

        debug!("Register handlebars helpers");
        renderer.register_hbs_helpers(&mut handlebars, &html_config);

        let mut data = make_data(&ctx.root, book, &ctx.config, &html_config, &theme)?;

        // Print version
        let mut print_content = String::new();

        fs::create_dir_all(destination)
            .with_context(|| "Unexpected error when constructing destination path")?;

        let mut is_index = true;
        // TODO: Store these in `self`.
        for item in book.iter() {
            let ctx = RenderItemContext {
                handlebars: &handlebars,
                destination: destination.to_path_buf(),
                data: data.clone(),
                is_index,
                book_config: book_config.clone(),
                html_config: html_config.clone(),
                edition: ctx.config.rust.edition,
                chapter_titles: &ctx.chapter_titles,
            };
            renderer.render_item(item, ctx, &mut print_content)?;
            // Only the first non-draft chapter item should be treated as the "index"
            is_index &= !matches!(item, BookItem::Chapter(ch) if !ch.is_draft_chapter());
        }

        // Render 404 page
        if html_config.input_404 != Some("".to_string()) {
            renderer.render_404(ctx, &html_config, &src_dir, &mut handlebars, &mut data)?;
        }

        // Print version
        renderer.configure_print_version(&mut data, &print_content);
        if let Some(ref title) = ctx.config.book.title {
            data.insert("title".to_owned(), json!(title));
        }

        // Render the handlebars template with the data
        if html_config.print.enable {
            debug!("Render template");
            let rendered = handlebars.render("index", &data)?;

            let rendered = renderer.post_process(
                rendered,
                &html_config.playground,
                &html_config.code,
                ctx.config.rust.edition,
            );

            utils::fs::write_file(destination, "print.html", rendered.as_bytes())?;
            debug!("Creating print.html ✓");
        }

        debug!("Copy static files");
        renderer
            .copy_static_files(destination, &theme, &html_config)
            .with_context(|| "Unable to copy across static files")?;
        renderer
            .copy_additional_css_and_js(&html_config, &ctx.root, destination)
            .with_context(|| "Unable to copy across additional CSS and JS")?;

        // Render search index
        let search = html_config.search.unwrap_or_default();
        if search.enable {
            search::create_files(&search, destination, book)?;
        }

        renderer
            .emit_redirects(&ctx.destination, &handlebars, &html_config.redirect)
            .context("Unable to emit redirects")?;

        // Copy all remaining files, avoid a recursive copy from/to the book build dir
        utils::fs::copy_files_except_ext(&src_dir, destination, true, Some(&build_dir), &["md"])?;

        Ok(())
    }
}
