### Fixed

- The `json-table` transform encodes a 1 MiB bare array about twice as fast as before. The encoder minifies the input once, builds the table block once for the table-v1 and table-v2 forms, and skips the sub-table rewrite for arrays without stringified JSON. Output bytes are unchanged (#79)
