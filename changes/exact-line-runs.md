### Added

- `deslag-core` adds the `lines-v1` exact line-run codec, off in `FIRST_SLICE_JSON_MINIFY_CONTEXT`, that encodes runs of identical consecutive lines in plain-text tool output, with an exact decoder. Input that already starts with the `lines-v1` header is skipped as `unsupported_format` (#13)
