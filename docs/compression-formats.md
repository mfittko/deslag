# Compression formats

This file specifies the text formats that deslag transforms emit. Each format has a local decoder in `deslag-core`.

## table-v1

`table-v1` is a lossless tabular encoding for a JSON array of objects. Uniform nested objects flatten to dotted columns. The encoder is `deslag_core::table::encode`. The decoder is `deslag_core::table::decode`. The transform identity is `json-table`, transform version 1, format version 1.

For every eligible input `x`, `decode(encode(x))` equals the minified form of `x` byte for byte. The minified form is the output of the JSON minifier: the same members in the same order, with every lexeme unchanged.

### Example

```text
table-v1 [3]{id:int,name:str,score:num,active:bool}
1,alpha,1.5,true
2,beta,2,false
3,gamma,null,true
```

This decodes to:

```json
[{"id":1,"name":"alpha","score":1.5,"active":true},{"id":2,"name":"beta","score":2,"active":false},{"id":3,"name":"gamma","score":null,"active":true}]
```

### Header grammar

The first line is the header. It has no leading or trailing whitespace.

```text
header  = "table-v1 [" count "]{" column *( "," column ) "}"
count   = "0" / %x31-39 *DIGIT
column  = path ":" type
path    = name / segment 1*( "." segment )
name    = *namechar
segment = 1*namechar
namechar = %x20-21 / %x23-2B / %x2D / %x2F-5B / %x5D-10FFFF
type    = "int" / "num" / "str" / "bool"
```

- `count` is the number of rows. It has no leading zeros.
- A `name` may be empty. A dotted `path` has two or more non-empty segments.
- Columns appear in first-seen key order across all rows.
- A decoder splits the column list on `,` and splits each column at its last `:`. A name may therefore contain `:`.
- A name or segment must not contain `.`, `,`, `"`, `\` or a control character below U+0020. Column paths are unique.

### Rows

Each row is one RFC 4180 record. Records are joined with `\n`. An empty record, for example a row `{}` in a one-column table, can therefore make the text end with `\n`. There are exactly `count` records, and each record has exactly one field per column.

A quoted field starts and ends with `"`. A `"` inside it is written as `""`. A quoted field may contain `,`, `\n` and `\r`. An unquoted field must not contain `"`, `,`, `\n` or `\r`.

### Column types

| Type | Non-null values |
| --- | --- |
| `int` | Every value is a JSON number with the lexeme `-?(0\|[1-9][0-9]*)`. |
| `num` | Every value is a JSON number, and at least one is not an `int` lexeme. |
| `str` | Every value is a JSON string. |
| `bool` | Every value is `true` or `false`. |

Any column may also hold `null` or a missing field. A column whose rows are all `null` or missing is typed `str`.

### Cells

| JSON field | Cell |
| --- | --- |
| Missing | Empty unquoted cell |
| `null` | Unquoted `null` |
| `true`, `false` | Unquoted `true`, `false` |
| Number | Unquoted original lexeme |
| String | The decoded text, quoted when required |

The encoder quotes a string cell when its text:

- is empty,
- is `null`, `true` or `false`,
- parses as a floating-point number under Rust `str::parse::<f64>` (this includes `1`, `-0`, `1e3`, `NaN` and `inf`), or
- contains `,`, `"`, `\n` or `\r`.

The decoder reads each cell as follows:

- A quoted cell is always a string. It is valid only in a `str` column.
- An empty unquoted cell is a missing field. It is valid only in an undotted column.
- An unquoted `null` is `null`.
- Any other unquoted cell is read by the column type. In an `int` column it must be an `int` lexeme. In a `num` column it must be a JSON number lexeme. In a `bool` column it must be `true` or `false`. In a `str` column it is the string text.

Any other cell makes the whole text invalid, and the decoder returns no value.

So `null`, a missing field and `""` stay distinct: they are `null`, an empty cell and `""`.

Encoder output is canonical. The decoder also accepts some non-canonical texts, for example a quoted `str` cell such as `"abc"` that needs no quotes, or a bare string such as `1` or `NaN` in a `str` column that the encoder would quote. Such texts decode, but encoding the result does not reproduce them.

### Nested objects

A key whose value is an object in any row flattens into dotted columns. The key must hold an object in every row, and every row's object must have the same key list in the same order. The object must not be empty. The rule applies recursively, so `{"a":{"b":{"c":1}}}` becomes the column `a.b.c`. The dotted columns take the key's position in the column order.

```text
table-v1 [2]{id:int,meta.region:str,meta.zone:int}
1,eu,3
2,us,null
```

This decodes to:

```json
[{"id":1,"meta":{"region":"eu","zone":3}},{"id":2,"meta":{"region":"us","zone":null}}]
```

A nested leaf may hold `null`, but it is never missing. Only a top-level field can be missing. Arrays stay ineligible at any depth. Nesting depth is bounded by `max_depth`.

The decoder splits each column path at `.`. The paths must form one tree, or the decoder returns no value:

- No path is both a leaf column and a prefix of another column, for example `a` and `a.b`.
- The columns under each object path are contiguous, for example `a.x,b,a.y` is invalid.
- No segment of a dotted path is empty, for example `.a`, `a.` and `a..b` are invalid.
- An empty unquoted cell in a dotted column is invalid.
- A column path has fewer than `MAX_JSON_DEPTH_CEILING` (1024) segments. This bounds decoder work for hostile headers.

### Lexeme preservation

Number lexemes are copied unchanged, for example `1.0`, `-0`, `1e3` and `12345678901234567890123`.

The decoder writes each string with one canonical encoding. It escapes `"` as `\"` and `\` as `\\`. It writes U+0008, U+000C, U+000A, U+000D and U+0009 as `\b`, `\f`, `\n`, `\r` and `\t`. It writes every other character below U+0020 as `\u00xx` with lowercase hex digits. It writes every other character literally, including `/` and non-ASCII characters.

A string is eligible only when its raw lexeme equals the canonical encoding of its text. An input with any other escape form, for example `\/`, `\u0041` or a surrogate pair, is ineligible.

Object keys are copied unchanged. Escaped keys are ineligible, so a key never needs an escape.

### Eligibility

The encoder returns the input unchanged, with a reason, in each of these cases:

| Case | Reason |
| --- | --- |
| The input is not a top-level JSON array | `unsupported_format` |
| The array has fewer than two items | `unsupported_format` |
| An item is not an object | `unsupported_format` |
| A field holds an array, at any depth | `unsupported_format` |
| A key holds an object in one row and is missing, `null` or a non-object in another row | `unsupported_format` |
| Two rows hold objects with different key lists, or a different key order, under the same key | `unsupported_format` |
| A field holds `{}` inside a row | `unsupported_format` |
| A key inside a nested object is empty, or a key whose value is an object is empty | `unsupported_format` |
| A column holds non-null values of more than one type (`int` and `num` together make a `num` column) | `unsupported_format` |
| A key contains `.`, `,`, `"`, `\` or a control character below U+0020 | `unsupported_format`, or `escaped_key` when the key needs an escape |
| An object has a duplicate key | `duplicate_key` |
| An object has an escaped key | `escaped_key` |
| A row's key order is not a subsequence of the column order | `unsupported_format` |
| No row has a key | `unsupported_format` |
| The array has more rows than `max_rows` | `unsupported_format` |
| The input, after leading whitespace, starts with `table-v1` | `unsupported_format` |
| A string lexeme is not canonical | `unsupported_format` |
| Containers nest deeper than `max_depth` | `depth_limit` |
| The gain check fails | `insufficient_gain` |

### Gain

The gain check compares the encoded text, header included, with the minified original. Let `m` be the minified byte length and `e` the encoded byte length. Encoding applies only when `e < m` and `(m - e) * 1000 >= min_savings_permille * m`. When a token counter is set, the minified original must also have at least `min_token_savings` more tokens than the encoded text.

`TableOptions::default()` sets `max_rows` to 20000, `min_savings_permille` to 100, `min_token_savings` to 64 and `max_depth` to 64, with no token counter.

### Markdown-KV (evaluation only)

`deslag_core::table::render_markdown_kv` renders the same table as a Markdown list. It exists so the evaluation harness can compare read-back accuracy against the CSV form. It has no decoder, and no `CompressionContext` setting selects it.

It applies the same eligibility rules and the same gain check as `encode`. The gain check runs on the CSV form. The renderer returns no value exactly when `encode` fails.

The output starts with the same header line as the CSV form. Each row follows as one list item:

- The first field is written as `- key: value`. Each further field is written on its own line as `  key: value`, indented two spaces.
- A nested leaf uses its dotted column path as the key, for example `meta.region`.
- A missing field is omitted. A row with no fields is a bare `-`.
- Each value is written exactly as its CSV cell, with the same quoting rules.

```text
table-v1 [2]{id:int,name:str,meta.region:str}
- id: 1
  name: ""
  meta.region: eu
- id: 2
  meta.region: "null"
```

### Pipeline and configuration

`compress` tries table-v1 only when `CompressionContext.json_table` is true. The switch is opt-in. It is false in `FIRST_SLICE_JSON_MINIFY_CONTEXT`, and its default is false.

When the switch is on, `compress` calls `encode` with `max_rows` set to `Limits::max_table_rows` (default 20000), `max_depth` set to `Limits::max_json_depth` and the context's token counter. The other thresholds keep their `TableOptions::default()` values. The size limits and mode checks apply first, as for the minifier.

- If the input is eligible, `compress` decodes the encoded text and compares it with the minified original. A mismatch fails with `invalid_candidate` and validation outcome `failed`. Otherwise the report has transform id `json-table`, transform version 1 and format version 1. The byte and token fields are filled as for the minifier. Observe mode emits the original with outcome `would_change`.
- If the input is ineligible and `json_minify` is true, `compress` runs the JSON minifier unchanged and reports it as `json-minify`.
- If the input is ineligible and `json_minify` is false, `compress` skips with the table-v1 reason and transform id `json-table`.
- If both switches are false, `compress` skips with `transform_disabled`.
