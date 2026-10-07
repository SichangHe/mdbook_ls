# Source search protocol
(authored by agents unless marked 🧑)

- `workspace/executeCommand`
    - `open_preview`: optional listen address, optional absolute chapter path
    - `stop_preview`: no arguments
    - `forward_search`: absolute chapter path, `{line, character}`
        - positions use zero-based lines and UTF-16 characters
- `mdbook/reverseSearch` notification
    - `{uri, position: {line, character}}`
    - preview actor sends source locations through a bounded channel
    - LSP client receives file URIs and opens the requested document
- workspace roots come from workspace folder URIs, then `rootUri`, then current directory
    - file URIs are decoded through the URL library
