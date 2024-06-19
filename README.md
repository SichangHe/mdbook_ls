# mdBook Language Server

WIP.
The goal of mdBook_LS is to provide a language server to
preview mdBook projects live.

## mdBook Incremental Preview

mdBook_incremental_preview provides incremental preview building for
mdBook projects.
Unlike `mdbook watch` or `mdbook serve`,
which are inefficient because they rebuild the whole book on file changes,
`mdBook_incremental_preview` only patches the changed chapters,
thus producing instant updates.

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
