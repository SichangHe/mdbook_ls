# mdBook Language Server

WIP.
The goal of mdBook LS is to provide a language server to
preview mdBook projects live.

## mdBook Incremental Preview

mdBook_Incremental_Preview provides incremental preview building for
mdBook projects.
Unlike `mdbook watch` or `mdbook serve`,
which are inefficient because they rebuild the whole book on file changes,
`mdBook_incremental_preview` only patches the changed chapters,
thus producing instant updates.

### Usage of mdBook Incremental Preview

At your project root, run:

```sh
mdbook_incremental_preview
```

It has basically the same functionality as `mdbook serve` but incremental:

- Chapter changes are patched individually.
    Full rebuilds happen only when the `.gitignore`, `book.toml`, `SUMMARY.md`,
    or the theme directory changes.
- Build artifacts are stored in a temporary directory in memory.
- It directly serves asset files from the source directory instead of
    copying all of them.

### Incremental preview current limitations

- Preprocessors that operate across multiple book item, like `link`,
    are not supported.
    The results may be incorrect,
    or the implementation may fall back to a full rebuild.
    This is because
    we feed the preprocessors the individual chapters rather than
    the whole book when patching.

    This limitation will hopefully be lifted in the future by
    whitelisting certain preprocessors to be fed with the whole book.

### Future work for incremental preview

- Do a full rebuild on manual page refresh.
- Make the `link` preprocessor work.

## Debugging

We use `tracing-subscriber` with the `env-filter` feature to
emit logs[^tracing-env-filter].
Please configure the log level by setting the `RUST_LOG` environment variable.

[^tracing-env-filter]: <https://docs.rs/tracing-subscriber/latest/tracing_subscriber/#feature-flags>
