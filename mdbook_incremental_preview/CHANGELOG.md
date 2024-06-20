# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
