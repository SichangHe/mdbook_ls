# Preview source search
(authored by agents unless marked 🧑)

source positions
- zero-based editor lines
- reverse search returns the enclosing top-level Markdown block's first line and character 0
- full renders read source files; incremental patches use editor content
- opened buffers are replayed after every full render
- preprocessing precedes marker insertion and the normal mdBook HTML renderer remains authoritative
- unchanged Markdown maps directly
- changed Markdown maps unique unchanged blocks
  - transformed or ambiguous blocks remain unmapped
  - includes and generated material lack guaranteed source positions

browser transport
- same-port WebSocket at `/__mdbook_source_search`
- Ctrl, Meta, or Alt click sends `{path,line}`
  - same-origin browser requests and registered chapters only
- editor forward search broadcasts `{url}` to connected previews
  - URL includes rendered chapter path and `#mdbook-source-line=N`
  - opens browser when no preview is connected
- `Previewer::with_reverse_search` sends `SearchLocation` through the supplied channel
- `PreviewInfo::ForwardSearch` resolves source path using the rendered chapter registry

limitations
- forward search scrolls to the nearest mapped block at or before the requested line
- browser navigation can reconnect during rebuilds
- generated raw HTML has no guaranteed mapping

validation
- `cargo test -p mdbook_incremental_preview --lib`
- `node mdbook_incremental_preview/tests/browser.cjs`
  - requires `playwright` in Node module lookup and its installed Chromium, or Chrome via `CHROME_BINARY`
  - `MDBOOK_LS_BINARY` and `CHROME_BINARY` optionally override executable locations
  - exercises real LSP, preview HTTP, browser DOM modifier clicks, forward navigation, and unsaved edits
