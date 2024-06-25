# mdBook Language Server

WIP.

mdBook-LS provides a language server to preview mdBook projects live,
patching the edited chapter instantly as you type in your editor.
Please see [Editor Setup](#editor-setup) for details on usage.

## Editor Setup

<details>
<summary>✅ NeoVim setup with LSPConfig</summary>

The plan is to merge this into [nvim-lspconfig].

Before that happens,
please paste the below `mdbook_ls_setup` function somewhere in
your configuration files and call it with your client's `capabilities`.

```lua
function mdbook_ls_setup(capabilities)
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
            arguments = { vim.uri_from_bufnr(0), true },
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

    require('lspconfig.configs')['mdbook_ls'] = {
        default_config = {
            cmd = { 'mdbook-ls' },
            filetypes = { 'markdown' },
            root_dir = lspconfig.util.root_pattern('book.toml'),
        },
        commands = {
            MDBookLSOpenPreview = {
                open_preview,
                description = 'Open MDBook-LS preview',
            },
            MDBookLSStopPreview = {
                stop_preview,
                description = 'Stop MDBook-LS preview',
            },
        },
        docs = {
            description = [[TODO]],
        },
    }
    lspconfig['mdbook_ls'].setup {
        capabilities = capabilities,
    }
end
```

Now, you would have two Vim commands:
`MDBookLSOpenPreview` starts the preview and opens the browser;
`MDBookLSStopPreview` stops updating the preview
(we are unable to stop the web server,
seemingly because of Warp's limitations).

</details>

<details>
<summary>❌ Visual Studio Code setup</summary>

I do not currently use VSCode,
do not plan to go through Microsoft's hoops to make an official plugin,
and do not wish to maintain such plugins.
If you use both VSCode and mdBook-LS,
please feel free to make a VSCode plugin yourself and create an issue so
I can link your plugin here.

</details>

## mdBook Incremental Preview

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

- Chapter changes are patched individually and pushed to browser.
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

### MathJax support

`MathJax.js` is too slow for live preview,
so you should instead consider [mdBook-KaTeX], [client-side KaTeX]
(with a custom script that listens to the `load` event, as mentioned above),
or other alternatives.

If you have to stick with MathJax,
add a custom script that listens to the `load` event and reruns MathJax,
like this:

```javascript
document.addEventListener("load", () => {
    if (MathJax?.Hub?.Typeset != undefined) {
        MathJax.Hub.Typeset();
    }
});
```

### Future work for incremental preview

- Background search indexing to save full rebuild time.

## Debugging

We use `tracing-subscriber` with the `env-filter` feature to
emit logs[^tracing-env-filter].
Please configure the log level by setting the `RUST_LOG` environment variable.

[^tracing-env-filter]: <https://docs.rs/tracing-subscriber/latest/tracing_subscriber/#feature-flags>

[client-side KaTeX]: https://katex.org/docs/browser.html
[load-event]: https://developer.mozilla.org/en-US/docs/Web/API/Window/load_event
[mdBook-KaTeX]: https://github.com/lzanini/mdbook-katex
[nvim-lspconfig]: https://github.com/neovim/nvim-lspconfig
