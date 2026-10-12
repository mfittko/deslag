### Added

- `deslag-core` table-v2 encodes a string cell whose text is minified JSON, such as an array of objects or an object that wraps such arrays, as a sub-table in a `json` column, and decodes it back to the same string lexeme. Sub-tables nest at most 4 levels. The `json-table` transform version is 4, because encoder output changes (#43)
