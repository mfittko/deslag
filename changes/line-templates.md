### Added

- `deslag-core` adds the `lines-v2` line-template codec, off in `FIRST_SLICE_JSON_MINIFY_CONTEXT` and enabled by `line_templates`, that encodes runs of consecutive plain-text lines differing only in timestamps, IDs, counts or paths as one template and a value table, with an exact decoder that also reads `lines-v1` (#42)
