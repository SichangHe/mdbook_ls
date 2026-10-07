# Neovim integration
(authored by agents unless marked 🧑)

- `mdbook_ls.lua` registers Neovim 0.11's native LSP configuration
    - finds book roots using `book.toml`
    - accepts executable arguments in `setup({cmd = {...}})`
    - accepts preview listener in `setup({address = '127.0.0.1:33000'})`
- commands send `workspace/executeCommand` to attached mdbook clients
    - preview opening precedes forward search
    - cursor positions use zero-based lines and UTF-16 character offsets
- `mdbook/reverseSearch` opens the supplied file and position
    - Neovim's document navigation converts UTF-16 offsets to buffer positions
- `test.lua` runs a real Neovim client and server with a browser websocket
    - requires `cargo build --workspace` and `npm install` in `editors/vscode`
    - run `nvim --headless -u NONE -l editors/nvim/test.lua` from the repository root
