# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.6](https://github.com/SichangHe/mdbook_ls/compare/mdbook_incremental_preview-v0.0.5...mdbook_incremental_preview-v0.0.6) - 2024-11-07

### Other

- update Cargo.toml dependencies

## [0.0.5](https://github.com/SichangHe/mdbook_ls/compare/mdbook_incremental_preview-v0.0.4...mdbook_incremental_preview-v0.0.5) - 2024-07-28

### Fixed
- post-processing in patch

## [0.0.4](https://github.com/SichangHe/mdbook_ls/compare/mdbook_incremental_preview-v0.0.3...mdbook_incremental_preview-v0.0.4) - 2024-06-28

### Added
- *(lsp)* ignore file watch event for opened files
- *(lsp)* open browser at opened chapter
- *(patching)* use `PatchJoinSets`
- *(patching)* `MDBookCore` to overtake&share preprocessing
- *(rebuilding)* two-JoinSet to keep oldest+newest rebuilds;

### Other
- reduce cloning w/ `Arc`
- `LivePatcher` → `Previewer`

## [0.0.3](https://github.com/SichangHe/mdbook_ls/compare/mdbook_incremental_preview-v0.0.2...mdbook_incremental_preview-v0.0.3) - 2024-06-23

### Added
- feat!(mdbook-incremental-preview): binary take arguments;rename;
- *(live patch)* issues `load` window event on patch
- *(live patch)* only replace `<main>`;rebuild on requesting patched page;

### Other
- *(clean up)* WebSocket connect on path to chapter,

## [0.0.2](https://github.com/SichangHe/mdbook_ls/compare/mdbook_incremental_preview-v0.0.1...mdbook_incremental_preview-v0.0.2) - 2024-06-21

### Added
- *(avoid copy)* directly serve static files&additional js&css
- *(reload file watcher)* precisely decide;watch `SUMMARY.md`
- *(incremental)* non-blocking async `rebuild_on_change`;
- *(incremental)* async `execute`&`JoinSet` shutdown
- *(incremental)* avoid copying assets in `src/`
- *(performance)* write build artifacts to temporary directory

### Fixed
- *(additional JS/CSS)* correctly serve from files

### Other
- *(release)* mdbook_fork4ls 0.4.41-patch.1
- *(server)* separate static file filters
- *(limitations)* `link` preprocessor does work

## [0.0.1](https://github.com/SichangHe/mdbook_ls/compare/mdbook_incremental_preview-v0.0.0...mdbook_incremental_preview-v0.0.1) - 2024-06-20

### Added
- *(incremental)* avoid copying assets in `src/`
- *(performance)* write build artifacts to temporary directory
- *(incremental)* configurable `execute`
- *(incremental)* patch individual chapter
- replicate handlebars renderer w/ caching;
- *(save rendering state)* copy `hbs_renderer` to `StatefulHtmlHbs` wrapper

### Fixed
- *(incremental)* write patch to correct path
- *(watching theme dir)* watch children
- *(incremental)* watch theme directory
- *(incremantal bin)* open browser after first build, socket address
- *(ci)* fetch submodules
- fix `notify` type import

### Other
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
