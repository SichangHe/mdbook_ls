use super::*;

#[derive(Default)]
pub struct HtmlHbsState {
    pub path2ctxs: HashMap<Arc<Path>, CtxCore>,
    /// Relative path of the source file of the index chapter.
    pub index_path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct CtxCore {
    pub chapter_name: Arc<str>,
    pub len_content: usize,
    pub html_path: PathBuf,
}

// NOTE: Below is adapted from
// <https://github.com/rust-lang/mdBook/blob/3bdcc0a5a6f3c85dd751350774261dbc357b02bd/src/renderer/html_handlebars/hbs_renderer.rs>.

pub fn html_config_n_theme_dir(ctx: &RenderContext) -> Result<(HtmlConfig, PathBuf)> {
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
    Ok((html_config, theme_dir))
}

impl HtmlHbsState {
    /// Render the book to HTML using the Handlebars renderer and
    /// save intermediate state.
    pub async fn full_render(&mut self, ctx: RenderContext) -> Result<()> {
        info!("Running the html backend for a full render.");
        let src_dir = ctx.source_dir();
        let destination = ctx.destination.clone();
        let book = &ctx.book;
        block_n_yield(|| HtmlHandlebars::new().render(&ctx)).await?;
        block_n_yield(|| inject_live_patch_script(&destination)).await?;

        let mut is_index = true;
        self.path2ctxs.clear();
        let items = || {
            book.iter().filter_map(|item| {
                if let BookItem::Chapter(Chapter {
                    name,
                    content,
                    path: Some(path),
                    source_path: Some(source_path),
                    ..
                }) = item
                {
                    Some((name, content, path, source_path))
                } else {
                    None
                }
            })
        };
        self.path2ctxs.reserve(items().count());

        for (name, content, path, source_path) in items() {
            let source_path = src_dir.join(source_path);
            let html_path = path.with_extension("html");
            if is_index {
                self.index_path = Some(html_path.clone());
            }
            // Only the first non-draft chapter item should be treated as the "index"
            is_index = false;
            let ctx = CtxCore {
                chapter_name: name.clone().into(),
                len_content: content.len(),
                html_path,
            };
            self.path2ctxs.insert(source_path.into(), ctx);
        }

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
    pub async fn patch<I: IntoIterator<Item = PathBuf>>(
        &self,
        book: &Arc<MDBookCore>,
        src_dir: &Arc<Path>,
        paths: I,
        patch_registry_ref: &ActorRef<PatchRegistry>,
        patch_join_sets: &mut PatchJoinSets,
    ) {
        for path in paths.into_iter() {
            if let Some((arc_path, ctx)) = self.path2ctxs.get_key_value(path.as_path()) {
                let task = patch_chapter(
                    arc_path.clone(),
                    ctx.clone(),
                    book.clone(),
                    src_dir.clone(),
                    patch_registry_ref.clone(),
                );
                _ = patch_join_sets.entry(path).or_default().spawn(task);
            };
        }
    }
}

pub async fn patch_chapter(
    path: Arc<Path>,
    CtxCore {
        chapter_name,
        len_content,
        html_path,
    }: CtxCore,
    book: Arc<MDBookCore>,
    src_dir: Arc<Path>,
    patch_registry_ref: ActorRef<PatchRegistry>,
) {
    let task = try_patch_chapter(
        &path,
        &chapter_name,
        len_content,
        &html_path,
        &src_dir,
        &book,
        &patch_registry_ref,
    );
    if let Err(err) = task.await {
        error!(
            ?err,
            ?path,
            chapter_name = chapter_name.as_ref(),
            "Patching chapter.",
        );
    }
}

pub async fn try_patch_chapter(
    path: &Path,
    chapter_name: &str,
    len_content: usize,
    html_path: &Path,
    src_dir: &Path,
    book: &MDBookCore,
    patch_registry_ref: &ActorRef<PatchRegistry>,
) -> Result<()> {
    let content = load_content_of_chapter(path, len_content * 2).await?;
    try_patch_chapter_w_content(
        path,
        src_dir,
        chapter_name,
        html_path,
        content,
        book,
        patch_registry_ref,
    )
    .await
}

pub async fn try_patch_chapter_w_content(
    path: &Path,
    src_dir: &Path,
    chapter_name: &str,
    html_path: &Path,
    content: String,
    book: &MDBookCore,
    patch_registry_ref: &ActorRef<PatchRegistry>,
) -> Result<()> {
    let relative_path = path.strip_prefix(src_dir)?;
    debug!(
        ?path,
        chapter_name,
        ?relative_path,
        "Patching with content.",
    );
    yield_now().await;
    let chapter = Chapter::new(chapter_name, content, relative_path, vec![]);
    let patcher_book = Book::new_with_items(vec![BookItem::Chapter(chapter)]);
    let (preprocessed_book, preprocess_ctx) = book.preprocess_book(patcher_book).await?;
    let patch_html = render_patch_html(
        book,
        preprocessed_book,
        preprocess_ctx,
        relative_path,
        html_path,
    )
    .await?;
    patch_registry_ref
        .cast(PatchRegistryRequest::NewPatch(
            html_path.to_path_buf(),
            patch_html,
        ))
        .await
        .context("Updating the patch registry")?;
    Ok(())
}

async fn render_patch_html(
    book: &MDBookCore,
    preprocessed_book: Book,
    preprocess_ctx: PreprocessorContext,
    relative_path: &Path,
    html_path: &Path,
) -> Result<String> {
    validate_patch_path(&preprocessed_book, relative_path, html_path)?;
    let temp_dir = tempdir().context("Creating temporary patch render directory")?;
    let mut render_context = RenderContext::new(
        book.root().to_path_buf(),
        preprocessed_book,
        book.config().clone(),
        temp_dir.path(),
    );
    render_context
        .chapter_titles
        .extend(preprocess_ctx.chapter_titles.borrow_mut().drain());
    block_n_yield(|| HtmlHandlebars::new().render(&render_context)).await?;
    let html_path = temp_dir.path().join(html_path);
    let html = fs::read_to_string(&html_path)
        .await
        .with_context(|| format!("Reading rendered patch {}", html_path.display()))?;
    extract_main_inner_html(&html).map(str::to_owned)
}

fn validate_patch_path(book: &Book, relative_path: &Path, html_path: &Path) -> Result<()> {
    match book.iter().next() {
        Some(BookItem::Chapter(Chapter {
            path: Some(path),
            source_path: Some(source_path),
            ..
        })) if source_path == relative_path && path.with_extension("html") == html_path => Ok(()),
        other => {
            bail!("Chapter at {relative_path:?} preprocessed to unexpected patch output {other:?}")
        }
    }
}

fn extract_main_inner_html(html: &str) -> Result<&str> {
    let start = html
        .find("<main")
        .and_then(|pos| html[pos..].find('>').map(|end| pos + end + 1))
        .context("Rendered patch did not contain `<main>`")?;
    let end = html[start..]
        .find("</main>")
        .map(|pos| start + pos)
        .context("Rendered patch did not contain `</main>`")?;
    Ok(html[start..end].trim())
}

fn inject_live_patch_script(destination: &Path) -> Result<()> {
    let script = format!(r#"<script src="/{LIVE_PATCH_PATH}"></script>"#);
    inject_live_patch_script_in_dir(destination, &script)
}

fn inject_live_patch_script_in_dir(dir: &Path, script: &str) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("Reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            inject_live_patch_script_in_dir(&path, script)?;
        } else if path.extension() == Some(OsStr::new("html")) {
            inject_live_patch_script_in_file(&path, script)?;
        }
    }
    Ok(())
}

fn inject_live_patch_script_in_file(path: &Path, script: &str) -> Result<()> {
    let html = std::fs::read_to_string(path)
        .with_context(|| format!("Reading rendered HTML {}", path.display()))?;
    if html.contains(script) {
        return Ok(());
    }
    let Some(body_end) = html.rfind("</body>") else {
        return Ok(());
    };
    let mut patched = String::with_capacity(html.len() + script.len());
    patched.push_str(&html[..body_end]);
    patched.push_str(script);
    patched.push_str(&html[body_end..]);
    std::fs::write(path, patched).with_context(|| format!("Writing {}", path.display()))
}

async fn load_content_of_chapter(path: &Path, capacity: usize) -> io::Result<String> {
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

/// A loaded [`MDBook`] that can preprocess temporary patch books.
pub struct MDBookCore {
    root: PathBuf,
    config: Config,
    source_dir: PathBuf,
    book: Mutex<MDBook>,
}

impl MDBookCore {
    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn source_dir(&self) -> &Path {
        &self.source_dir
    }

    /// Run preprocessors on `book` and return the final book.
    pub async fn preprocess_book(&self, book: Book) -> Result<(Book, PreprocessorContext)> {
        block_n_yield(|| {
            let mut mdbook = self
                .book
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let guard = BookRestoreGuard::new(&mut mdbook, book);
            guard.preprocess()
        })
        .await
    }
}

struct BookRestoreGuard<'a> {
    mdbook: &'a mut MDBook,
    original_book: Option<Book>,
}

impl<'a> BookRestoreGuard<'a> {
    fn new(mdbook: &'a mut MDBook, book: Book) -> Self {
        let original_book = mem::replace(&mut mdbook.book, book);
        Self {
            mdbook,
            original_book: Some(original_book),
        }
    }

    fn preprocess(mut self) -> Result<(Book, PreprocessorContext)> {
        let result = self.mdbook.preprocess_book(&HtmlHandlebars::new());
        self.restore();
        result
    }

    fn restore(&mut self) {
        if let Some(original_book) = self.original_book.take() {
            self.mdbook.book = original_book;
        }
    }
}

impl Drop for BookRestoreGuard<'_> {
    fn drop(&mut self) {
        self.restore();
    }
}

impl From<MDBook> for MDBookCore {
    fn from(value: MDBook) -> Self {
        let source_dir = value.root.join(&value.config.book.src);
        let config = value.config.clone();
        let root = value.root.clone();
        Self {
            root,
            config,
            source_dir,
            book: Mutex::new(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_main_with_attributes() {
        let html = r#"<html><body><main id="content"><h1>x</h1></main></body></html>"#;
        assert_eq!(extract_main_inner_html(html).unwrap(), "<h1>x</h1>");
    }

    #[test]
    fn validates_readme_patch_output_as_index() {
        let mut chapter = Chapter::new("intro", String::new(), "README.md", vec![]);
        chapter.path = Some("index.md".into());
        let book = Book::new_with_items(vec![BookItem::Chapter(chapter)]);

        validate_patch_path(&book, Path::new("README.md"), Path::new("index.html")).unwrap();
        validate_patch_path(&book, Path::new("README.md"), Path::new("README.html")).unwrap_err();
    }
}
