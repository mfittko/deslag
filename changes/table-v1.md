### Added

- `deslag-core` adds the `table-v1` encoder and decoder for arrays of JSON objects, with uniform nested objects flattened to dotted columns and eligibility and gain checks. `compress` uses it only when the opt-in `json_table` switch is on, bounded by `Limits::max_table_rows` and falling back to the JSON minifier. An evaluation-only Markdown-KV formatter renders the same table. The format is specified in `docs/compression-formats.md` (#12)
