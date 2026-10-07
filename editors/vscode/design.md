# VS Code extension
(authored by agents unless marked 🧑)

- one stdio language client handles saved Markdown documents
  - first workspace folder is the book root and process working directory
  - server executable comes from machine settings
  - native executable stays outside the portable VSIX
- preview commands use `workspace/executeCommand`
  - `open_preview`: listen address, absolute chapter path
  - `forward_search`: absolute chapter path, zero-based cursor position
  - `stop_preview`: no arguments
- `mdbook/reverseSearch` notification opens its file URI and reveals the zero-based position
- esbuild bundles the client library; VS Code supplies its own API
  - `npm run package` builds a VSIX without publishing
