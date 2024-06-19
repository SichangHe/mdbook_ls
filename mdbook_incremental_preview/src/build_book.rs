use super::*;

pub fn config_book_for_live_reload(book: &mut MDBook) -> Result<()> {
    book.config
        .set("output.html.live-reload-endpoint", LIVE_RELOAD_ENDPOINT)
        .context("live-reload-endpoint update failed")?;
    // Override site-url for local serving of the 404 file
    book.config.set("output.html.site-url", "/")?;
    Ok(())
}

// NOTE: Below is adapted from
// <https://github.com/rust-lang/mdBook/blob/3bdcc0a5a6f3c85dd751350774261dbc357b02bd/src/book/mod.rs>.

pub fn make_render_context(book: &MDBook, build_dir: &Path) -> Result<RenderContext> {
    // We only run the HTML renderer.
    let (preprocessed_book, preprocess_ctx) = book.preprocess_book(&RENDERER)?;

    let mut render_context = RenderContext::new(
        book.root.clone(),
        preprocessed_book,
        book.config.clone(),
        build_dir,
    );
    render_context
        .chapter_titles
        .extend(preprocess_ctx.chapter_titles.borrow_mut().drain());
    Ok(render_context)
}
