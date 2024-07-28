# mdBook Language Server

mdBook-LS provides a language server to preview mdBook projects live,
patching the edited chapter instantly and asynchronously as you type in
your editor.

## mdBook-LS Features

<https://github.com/SichangHe/mdbook_ls/assets/84777573/f75eb653-a143-4191-9c87-e6cb6064e6bc>

- **Live preview**: Instantly see the latest preview as you type in the editor.
- **Asynchronous patching**: No blocking your editor; under high load,
    always tries to render the latest version while
    showing intermediate feedbacks, using [a two-JoinSet].
- **Peripheral watching**:
    Change the important files of your project (`.gitignore`, `book.toml`,
    `SUMMARY.md`, and the theme directory) and see the book fully rebuilt;
    it reloads the file watcher and the web server as needed.
- Refresh a patched page to manually trigger a full rebuild.

## Editor Setup

<details><summary>Installation with, e.g., Cargo.</summary>

```sh
cargo install mdbook_ls
```

</details>

### ✅ NeoVim setup with LSPConfig

Please paste the below `register_mdbook_ls` function in
your Nvim configuration, call it,
and then set up `mdbook_ls` like any other LSPConfig language server.
[Please see my config for an
example](https://github.com/SichangHe/.config/blob/a01e81bb84dd24ef350882e912d56feb1c3ef9db/nvim/lua/plugins/lsp.lua#L256).

The snippet provides two Vim commands:
`MDBookLSOpenPreview` starts the preview (if not already started)
and opens the browser at the chapter you are editing;
`MDBookLSStopPreview` stops updating the preview
(Warp may keep serving on the port despite being cancelled).

<details>
<summary>The <code>mdbook_ls_setup</code> function.</summary>

```lua
local function register_mdbook_ls()
    local lspconfig = require('lspconfig')
    local function execute_command_with_params(params)
        local clients = lspconfig.util.get_lsp_clients {
            bufnr = vim.api.nvim_get_current_buf(),
            name = 'mdbook_ls',
        }
        for _, client in ipairs(clients) do
            client.request('workspace/executeCommand', params, nil, 0)
        end
    end
    local function open_preview()
        local params = {
            command = 'open_preview',
            arguments = { "127.0.0.1:33000", vim.api.nvim_buf_get_name(0) },
        }
        execute_command_with_params(params)
    end
    local function stop_preview()
        local params = {
            command = 'stop_preview',
            arguments = {},
        }
        execute_command_with_params(params)
    end

    require('lspconfig.configs').mdbook_ls = {
        default_config = {
            cmd = { 'mdbook-ls' },
            filetypes = { 'markdown' },
            root_dir = lspconfig.util.root_pattern('book.toml'),
        },
        commands = {
            MDBookLSOpenPreview = {
                open_preview,
                description = 'Open mdBook-LS preview',
            },
            MDBookLSStopPreview = {
                stop_preview,
                description = 'Stop mdBook-LS preview',
            },
        },
        docs = {
            description = [[The mdBook Language Server for previewing mdBook projects live.]],
        },
    }
end
```

</details>

I plan to merge this into [nvim-lspconfig] in the future.

### ❓ Visual Studio Code and other editor setup

<details>
<summary>No official support, but community plugins are welcome.</summary>

I do not currently use VSCode and these other editors,
so I do not wish to maintain plugins for them.

However,
it should be straightforward to implement plugins for them since
mdBook-LS implements the Language Server Protocol (LSP).
So,
please feel free to make a plugin yourself and create an issue for me to
link it here.

</details>

## mdBook Incremental Preview

mdBook-Incremental-Preview powers the live preview feature of mdBook-LS.
It can also be used standalone if you only wish to update the preview on
file saves.

mdBook-Incremental-Preview provides incremental preview building for
mdBook projects.
Unlike `mdbook watch` or `mdbook serve`,
which are inefficient because they rebuild the whole book on file changes,
`mdBook-incremental-preview` only patches the changed chapters,
thus producing instant updates.

### Usage of mdBook Incremental Preview

At your project root, run:

```sh
mdbook-incremental-preview
```

It has basically the same functionality as `mdbook serve` but incremental:

- Chapter changes are patched individually and pushed to the browser,
    without refresh.
- Full rebuilds happen only when the `.gitignore`, `book.toml`, `SUMMARY.md`,
    or the theme directory changes,
    or a patched page is requested by a new client.
    <!-- NOTE: We need to rebuild on theme changes because of templates. -->
- Build artifacts are stored in a temporary directory in memory.
- It directly serves static files, additional JS & CSS,
    and asset files from the source directory, instead of copying them.

### Details of patching

When a chapter changes,
we push its patched content to the corresponding browser tabs and
replace the contents of their `<main>` elements.
So, the browser does not reload the page, but updates the content instantly.

After replacing the content,
our injected script issues a [`load` window event][load-event].
You should listen to this event to rerun any JavaScript code as needed.
An example is below in [the MathJax support section](#mathjax-support).

### Current limitations of patching

- Preprocessors that operate across multiple book item are not supported.
    The results may be incorrect,
    or the implementation may fall back to a full rebuild.
    This is because
    we feed the preprocessors the individual chapters rather than
    the whole book when patching.

    This is irrelevant for most preprocessors,
    which operate on a single chapter.
    Even the `link` preprocessor works because
    it reads the input files directly.
- Neither `print.html` or the search index are updated incrementally.
    They are only rebuilt on full rebuilds,
    which can be triggered by refreshing a patched page.
- The book template (`index.hbs`)
    has to include exactly `{{ content }}` in the `<main>` tag (the default),
    otherwise the patching will not work correctly.
    A workaround would be to allow custom injected scripts,
    but I will not implement that unless demanded.

### MathJax support

`MathJax.js` is too slow for live preview,
so you should instead consider [mdBook-KaTeX], [client-side KaTeX]
(with a custom script that listens to the `load` event, as mentioned above),
or other alternatives.

If you have to stick with MathJax,
please add a custom script that listens to the `load` event and reruns MathJax,
like this:

```javascript
document.addEventListener("load", () => MathJax.Hub.Typeset());
```

## Debugging

We use `tracing-subscriber` with the `env-filter` feature to
emit logs[^tracing-env-filter].
Please configure the log level by setting the `RUST_LOG` environment variable.

## Contributing

I welcome high-quality issues and pull requests.

## Future work

- Unit tests so I do not need to test it in the editor on every commit.
- Integrate with Open Telemetry so I do not need to stare at all the logs.

[^tracing-env-filter]: <https://docs.rs/tracing-subscriber/latest/tracing_subscriber/#feature-flags>

[a two-JoinSet]: https://docs.rs/tokio_two_join_set/latest/tokio_two_join_set/struct.TwoJoinSet.html
[client-side KaTeX]: https://katex.org/docs/browser.html
[load-event]: https://developer.mozilla.org/en-US/docs/Web/API/Window/load_event
[mdBook-KaTeX]: https://github.com/lzanini/mdbook-katex
[nvim-lspconfig]: https://github.com/neovim/nvim-lspconfig
