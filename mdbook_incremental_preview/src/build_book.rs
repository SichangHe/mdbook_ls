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
