# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.1](https://github.com/SichangHe/mdbook_ls/compare/mdbook_ls-v0.0.0...mdbook_ls-v0.0.1) - 2024-06-27

### Added
- *(lsp)* open browser at opened chapter
- *(patching)* use `PatchJoinSets`
- *(patching)* `MDBookCore` to overtake&share preprocessing
- *(rebuilding)* two-JoinSet to keep oldest+newest rebuilds;
- Initial LSP implementation ([#3](https://github.com/SichangHe/mdbook_ls/pull/3))
- feat!(mdbook-incremental-preview): binary take arguments;rename;
- *(live patch)* issues `load` window event on patch
- *(live patch)* only replace `<main>`;rebuild on requesting patched page;
- *(avoid copy)* directly serve static files&additional js&css
- *(reload file watcher)* precisely decide;watch `SUMMARY.md`
- *(incremental)* non-blocking async `rebuild_on_change`;
- *(incremental)* async `execute`&`JoinSet` shutdown
- *(incremental)* avoid copying assets in `src/`
- *(performance)* write build artifacts to temporary directory
- *(incremental)* configurable `execute`
- *(incremental)* patch individual chapter
- replicate handlebars renderer w/ caching;
- *(save rendering state)* copy `hbs_renderer` to `StatefulHtmlHbs` wrapper

### Fixed
- *(additional JS/CSS)* correctly serve from files
- *(incremental)* write patch to correct path
- *(watching theme dir)* watch children
- *(incremental)* watch theme directory
- *(incremantal bin)* open browser after first build, socket address
- *(ci)* fetch submodules
- fix `notify` type import

### Other
- `LivePatcher` → `Previewer`
- *(release)* `mdbook_incremental_preview` v0.0.3
- *(clean up)* WebSocket connect on path to chapter,
- *(release)* mdbook_incremental_preview 0.0.2
- *(release)* mdbook_fork4ls 0.4.41-patch.1
- *(server)* separate static file filters
- *(limitations)* `link` preprocessor does work
- release
- explain incremental preview usage&debugging
- *(incremental)* future work
- only pass in book root not book if possible
- reuse `mdbook` code via `mdbook_fork4ls`
- move book building to `rebuild_on_change`
- copy (mostly) `HtmlHandlebars` renderer for full rendering control
- basics for incremental updates: detect gitignore/config change,
- extract book building
- split source
- avoid double book init
- get file change event not paths;reuse gitingore
- copy mdbook serve into mdbook_incremental_preview
