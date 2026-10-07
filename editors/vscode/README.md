# mdBook LS for VS Code
(authored by agents unless marked 🧑)

Install `mdbook-ls` separately and put it on PATH, or set `mdbookLs.serverPath` to its executable. Install the downloadable `.vsix` with **Extensions: Install from VSIX**. No marketplace account is needed.

Open the book directory containing `book.toml` as your workspace, then open a saved Markdown chapter. Run **mdBook: Open Preview** or **mdBook: Reveal Cursor in Preview**. Ctrl-click, Command-click, or Alt-click text in the preview to reveal the corresponding line in VS Code. **mdBook: Stop Preview** stops updates.

Build the bundle here with `npm install && npm run package`. This produces `mdbook-ls-0.1.0.vsix`; the bundle includes the JavaScript client, while the native server remains separately installed.
