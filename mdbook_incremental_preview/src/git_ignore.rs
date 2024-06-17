use super::*;

pub fn maybe_make_gitignore(book_root: &Path) -> Option<(Gitignore, PathBuf)> {
    find_ignore_path(book_root).map(|gitignore_path| {
        let (ignore, err) = Gitignore::new(&gitignore_path);
        if let Some(err) = err {
            warn!(?err, ?gitignore_path, "reading gitignore",);
        }
        let ignore_root = ignore
            .path()
            .canonicalize()
            .expect("ignore root canonicalize error");
        (ignore, ignore_root)
    })
}

pub fn find_ignore_path(book_root: &Path) -> Option<PathBuf> {
    book_root
        .ancestors()
        .map(|p| p.join(".gitignore"))
        .find(|p| p.exists())
}

// Note: The usage of `canonicalize` may encounter occasional failures on the Windows platform, presenting a potential risk.
// For more details, refer to [Pull Request #2229](https://github.com/rust-lang/mdBook/pull/2229#discussion_r1408665981).
pub fn is_ignored_file(ignore: &Gitignore, ignore_root: &Path, path: &Path) -> bool {
    let relative_path =
        pathdiff::diff_paths(path, ignore_root).expect("One of the paths should be an absolute");
    ignore
        .matched_path_or_any_parents(&relative_path, relative_path.is_dir())
        .is_ignore()
}
