# Compression formats

This file specifies the text formats that deslag transforms emit. Each format has a local decoder in `deslag-core`.

## table-v1

`table-v1` is a lossless tabular encoding for a JSON array of flat objects. The encoder is `deslag_core::table::encode`. The decoder is `deslag_core::table::decode`. The transform identity is `json-table`, transform version 1, format version 1.

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
count   = 1*DIGIT
column  = name ":" type
type    = "int" / "num" / "str" / "bool"
```

- `count` is the number of rows.
- Columns appear in first-seen key order across all rows.
- A decoder splits the column list on `,` and splits each column at its last `:`. A name may therefore contain `:`.
- A name must not contain `.`, `,`, `"`, `\` or a control character below U+0020. Column names are unique.

### Rows

Each row is one RFC 4180 record. Records are separated by `\n`. The text has no trailing newline. There are exactly `count` records, and each record has exactly one field per column.

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
- An empty unquoted cell is a missing field.
- An unquoted `null` is `null`.
- Any other unquoted cell is read by the column type. In an `int` column it must be an `int` lexeme. In a `num` column it must be a JSON number lexeme. In a `bool` column it must be `true` or `false`. In a `str` column it is the string text.

Any other cell makes the whole text invalid, and the decoder returns no value.

So `null`, a missing field and `""` stay distinct: they are `null`, an empty cell and `""`.

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
| A field holds an array or an object | `unsupported_format` |
| A column holds non-null values of more than one type (`int` and `num` together make a `num` column) | `unsupported_format` |
| A key contains `.`, `,`, `"`, `\n` or `\r` | `unsupported_format`, or `escaped_key` when the key needs an escape |
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
