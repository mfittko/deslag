### Added

- `deslag-core` adds the `table-v2` format for arrays of objects inside a wrapper object, such as `{"results":[...],"total":1000}`: the wrapper's other members stay byte-identical in a skeleton line and each eligible member array becomes a table-v1-style block. With `json_table` on, `compress` emits it with format version 2, keeps table-v1 output for bare top-level arrays (still format version 1), and validates with a decoder that reads both versions (#43)
