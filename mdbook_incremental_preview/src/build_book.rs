use mdbook::{renderer::RenderContext, Renderer};

use self::hbs_renderer::HtmlHandlebars;

use super::*;

pub fn config_and_build_book(book: &mut MDBook) -> Result<()> {
    config_book_for_live_reload(book)?;
    book.build()
}

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

pub fn full_build(book: &MDBook) -> Result<()> {
    let renderer = HtmlHandlebars;
    let (preprocessed_book, preprocess_ctx) = book.preprocess_book(&renderer)?;

    let name = renderer.name();
    let build_dir = book.build_dir_for(name);
    let mut render_context = RenderContext::new(
        book.root.clone(),
        preprocessed_book,
        book.config.clone(),
        build_dir,
    );
    let chapter_titles = todo!();

    Ok(())
}
