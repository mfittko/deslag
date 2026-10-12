# Compression formats

Formats that deslag transforms emit. Each format is versioned. A decoder for a
version accepts every output that version's encoder can produce. Each format
has a local decoder in `deslag-core`.

## lines-v1

Exact codec for runs of byte-identical consecutive lines in plain text. The
decoder reconstructs the original bytes exactly. Transform identity:
`exact-line-runs`, transform version 1, report `format_version` 1.

### Units

- A unit is a maximal byte sequence that ends with `\n`, or the final bytes
  after the last `\n`.
- Only `\n` ends a unit. A `\r` before `\n` belongs to the unit content.
- A partial final unit has no terminator, so `a\n` and `a` are different units
  and never form one run.
- Empty input has no units.

### Encoded text

The encoded text is the header line followed by the body.

- Header: `#!deslag lines-v1\n`.
- Reserved prefix: `#!deslag`. Every body line that starts with the prefix is
  a run directive or an escaped line. Any other body line is a literal unit.
- Run directive: `#!deslag x<N>\n`. N is canonical decimal with no leading
  zero and N >= 2. It means: the unit directly above occurs N times in total.
  A directive always ends with `\n`. It directly follows a literal or escaped
  unit. Two directives never follow each other.
- Escape: an original unit that starts with the reserved prefix is written as
  `#!deslag|` followed by the unit bytes. The decoder removes the first
  `#!deslag|` bytes. This covers header lookalikes, directive lookalikes and a
  partial final unit that starts with the prefix.

### Decoder

- The input must start with the header. Otherwise it is rejected.
- A body line that starts with the prefix and is neither a valid escape nor a
  canonical run directive is rejected. So is a directive with no unit directly
  above it, and an escape whose unit does not start with the prefix.
- The caller passes a maximum output size in bytes. The decoder fails before
  the output would exceed it, so a crafted count cannot allocate without bound.

### Encoder

- Units are compared byte for byte, including the terminator. Only
  consecutive units form a run. Order never changes and no unit is dropped.
- A run is encoded only when the directive is shorter than the emitted bytes
  of the N-1 copies it replaces. A copy of a unit that starts with the prefix
  is emitted with its escape. Otherwise the copies are written out.
- The candidate is emitted only when it is shorter than the input
  (`insufficient_gain` otherwise).
- Input that already starts with the header is skipped as
  `unsupported_format`, so the codec never encodes its own output.

### Integration

- The transform is opt-in: `CompressionContext::exact_line_runs` is `false` in
  `FIRST_SLICE_JSON_MINIFY_CONTEXT`.
- With the switch on, input that the JSON scanner rejects as
  `unsupported_format` is plain text and goes to this codec. With
  `json_minify` on, JSON content stays on the JSON path. With `json_minify` off
  and `exact_line_runs` on, input that the table encoder (table-v1 or table-v2)
  rejects as `unsupported_format` goes to this codec, or all input when
  `json_table` is off. Other table encoder rejections (`insufficient_gain`, `duplicate_key`, `escaped_key`,
  `depth_limit`) skip with that reason and the `json-table` id (see table-v1,
  Pipeline and configuration). When `line_templates` is on, it takes
  precedence and the input goes to the `lines-v2` encoder instead.
- The JSON scanner checks key policy and depth before it checks that the whole
  input is one value. Input whose leading JSON value it rejects for
  `duplicate_key`, `escaped_key` or `depth_limit` stays on the JSON path and
  reports that reason, even when lines that are not JSON follow.
- The core decodes the candidate and compares it with the input. A mismatch
  yields `failed` / `invalid_candidate`.
- The prefix, escape, run directive and unit splitting are shared with
  `lines-v2`.

## lines-v2

Exact codec for runs of consecutive lines that share a template, such as log
lines that differ only in timestamps, IDs, counts or paths. `lines-v2` is
`lines-v1` plus the template directive. The decoder reconstructs the original
bytes exactly. Transform identity: `line-templates`, transform version 1,
report `format_version` 2.

### Units and tokens

- Units are as in `lines-v1`.
- The template directive applies only to units that end with `\n`. The content
  is the unit without that final `\n`. A `\r` before it belongs to the content.
- Space and tab are the only delimiters. The content splits into alternating
  gaps and tokens: `gap0 tok0 gap1 ... tok(n-1) gapn`. Only the first and the
  last gap may be empty. Tokens are never empty.
- A token is variable-class when it contains an ASCII digit, `/` or `\`.

### Encoded text

The encoded text is the header line followed by the body.

- Header: `#!deslag lines-v2\n`.
- The reserved prefix, the escape and the run directive are as in `lines-v1`.
- Template directive: `#!deslag t<N> <p1>,<p2>,...\n`. N is canonical decimal
  and N >= 2. It means: the unit directly above is the base, and the group has
  N lines in total, the base included. The base is a literal or escaped unit
  and ends with `\n`.
- Each `p` is a canonical decimal token index of the base. Index 0 is the
  first token, and `0` is the only number that may start with `0`. There is
  at least one index. The indices ascend strictly, and each is below the
  base's token count.
- N-1 rows follow the directive, one per line of the group after the base.
  A row holds that line's tokens at the listed indices, in index order,
  joined by one space and followed by `\n`. Values are non-empty and contain
  no space or tab.
- A row that starts with the reserved prefix is written with the escape.
- A run directive or a template directive never directly follows a template
  group.

### Decoder

- The decoder accepts both headers. Under the `lines-v1` header a template
  directive is rejected, so every `lines-v1` text decodes as before.
- Each row line is the base with the row's values in place of the tokens at
  the listed indices. The gaps and the other tokens come from the base.
- The decoder rejects a template directive that is not canonical, has no base
  directly above it, has indices that do not ascend or are out of range, or
  lacks rows. It also rejects a row with the wrong number of values, an empty
  value, a tab in a value, a missing `\n`, or the reserved prefix without the
  escape.
- The decoder does not check that some row differs from the base at each
  listed index, or that each listed token is variable-class.
- The output bound applies as in `lines-v1`.

### Encoder

At each unit, in input order:

1. Count the identical consecutive copies. When there are at least 2 and the
   `lines-v1` run rule pays, write the unit and a run directive.
2. Otherwise grow a group greedily from this unit as the base. A following
   unit joins when it ends with `\n`, has the same gaps byte for byte and the
   same token count as the base, and each of its tokens equals the base token
   or both tokens are variable-class. A constant token that differs ends the
   group. So lines that differ in a constant token (a token with no ASCII
   digit, `/` or `\`) never share a template.
3. The variable indices are the indices where any row differs from the base.
   The group is written only when the directive and the rows are shorter than
   the rows' literal bytes, escapes included. Otherwise the base is written
   literally. The encoder keeps two values: `no_group_before` and
   `failed_end`. A group attempt runs at unit i only when i is at least
   `no_group_before`. `failed_end` is the end of the span of the most recent
   failed group with two or more lines. When such a group fails at unit i and
   i is before `failed_end`, `no_group_before` becomes the larger of the two
   span ends and `failed_end` is cleared. Otherwise `failed_end` becomes the
   end of the failed group's span. A failed one-line attempt (no row fits the
   base), a run directive and a written group change neither value. Run
   directives still apply to all units.

- Line order never changes and no line is dropped.
- A partial final unit is always literal.
- The candidate is emitted only when it is shorter than the input
  (`insufficient_gain` otherwise).
- Input that already starts with the `lines-v1` or `lines-v2` header is
  skipped as `unsupported_format`.

### Integration

- The transform is opt-in: `CompressionContext::line_templates` is `false` in
  `FIRST_SLICE_JSON_MINIFY_CONTEXT`.
- Routing is as for `exact_line_runs`. With `line_templates` on, plain text
  goes to this encoder, which also writes run directives. With only
  `exact_line_runs` on, the codec stays `lines-v1`.
- The core decodes the candidate and compares it with the input. A mismatch
  yields `failed` / `invalid_candidate`.

## table-v1

`table-v1` is a lossless tabular encoding for a JSON array of objects. Uniform nested objects flatten to dotted columns. The encoder is `deslag_core::table::encode`. The decoder is `deslag_core::table::decode`. The transform identity is `json-table`, format version 1. The transform version was 1 when table-v1 was the only table format. It was 2 with table-v2 wrapped arrays, 3 with bucket blocks, and is 4 since sub-tables. `compress` still emits table-v1 for a bare top-level array unless a table-v2 block is shorter (see table-v2).

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

With `count` 0 the text is the header line only, with no trailing `\n`. Otherwise the header is followed by `\n` and the records. Each row is one RFC 4180 record. Records are joined with `\n`. An empty record, for example a row `{}` in a one-column table, can therefore make the text end with `\n`. There are exactly `count` records, and each record has exactly one field per column.

A quoted field starts and ends with `"`. A `"` inside it is written as `""`. A quoted field may contain `,`, `\n` and `\r`. An unquoted field must not contain `"`, `,`, `\n` or `\r`.

### Column types

| Type | Non-null values |
| --- | --- |
| `int` | Every value is a JSON number with the lexeme `-?(0\|[1-9][0-9]*)`. |
| `num` | Every value is a JSON number, and at least one is not an `int` lexeme. |
| `str` | Every value is a JSON string. |
| `bool` | Every value is `true` or `false`. |

Any column may hold `null`. Only an undotted column may hold a missing field. A column whose rows are all `null` or missing is typed `str`.

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

A nested leaf may hold `null`. Only a top-level field can be missing. Arrays stay ineligible at any depth. Nesting depth is bounded by `max_depth`.

The decoder splits each column path at `.`. The paths must form one tree, or the decoder returns no value:

- No path is both a leaf column and a prefix of another column, for example `a` and `a.b`.
- The columns under each object path are contiguous, for example `a.x,b,a.y` is invalid.
- No segment of a dotted path is empty, for example `.a`, `a.` and `a..b` are invalid.
- An empty unquoted cell in a dotted column is invalid.
- A column path has fewer than `MAX_JSON_DEPTH_CEILING` (1024) segments.

The decoder tracks each object by its parent and its name, so header parsing is linear in header length.

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
| The distinct column paths together are longer than the minified input | `unsupported_format` |
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
- A value with line breaks continues on further lines. Each line break (`\r\n`, a lone `\r` or `\n`) is written as `\n`, and each line after the first is indented two spaces, the item indent. Value text therefore cannot start a new top-level row item. An indented continuation line can still read as a nested list item inside the row.

The example below is illustrative. It is rendered with `min_savings_permille: 0`; the default gain check rejects an input this small.

```text
table-v1 [2]{id:int,name:str,meta.region:str}
- id: 1
  name: ""
  meta.region: eu
- id: 2
  meta.region: "null"
```

### Pipeline and configuration

`compress` tries the table encoder only when `CompressionContext.json_table` is true. It calls `table::v2::encode`, which emits table-v1 for a bare top-level array unless a bucket block is shorter, and table-v2 otherwise, and validates with `table::v2::decode`, which reads both. The switch is opt-in. It is false in `FIRST_SLICE_JSON_MINIFY_CONTEXT`.

When the switch is on, `compress` calls `encode` with `max_rows` set to `Limits::max_table_rows` (default 20000), `max_depth` set to `Limits::max_json_depth` and the context's token counter. The other thresholds keep their `TableOptions::default()` values. The size limits and mode checks apply first, as for the minifier.

- If the input is eligible, `compress` decodes the encoded text and compares it with the minified original. A mismatch fails with `invalid_candidate` and validation outcome `failed`. Otherwise the report has transform id `json-table`, transform version 4 and format version 1, or format version 2 for table-v2 text. The byte and token fields are filled as for the minifier. Observe mode emits the original with outcome `would_change`.
- If the input is ineligible and `json_minify` is true, `compress` runs the JSON minifier unchanged and reports it as `json-minify`.
- If the input is ineligible and `json_minify` is false, `compress` skips with the table encoder's reason and transform id `json-table`. The format version is `null`, because no candidate was built.
- If `exact_line_runs` or `line_templates` is true, input that the JSON transforms reject as `unsupported_format` goes to the line codec as described in its Integration section. The report has transform id `line-templates` when `line_templates` is true. Otherwise it has transform id `exact-line-runs`. The report names one selected transform: the table encoder (table-v1 or table-v2), else the minifier, else the line codec. More than one transform can run. When `json_minify` is true, input that the table encoder rejects falls through to the minifier. An eligible table encoder candidate is always validated against the minified original.
- If `json_table` is true and `json_minify` is false, every other skip reports transform id `json-table`, including the mode and size checks that run before encoding.
- If `json_table`, `json_minify`, `exact_line_runs` and `line_templates` are all false, `compress` skips with `transform_disabled`.

## table-v2

`table-v2` encodes a JSON document whose arrays of objects sit inside a wrapper object, for example `{"results":[...],"total":1000}`, and arrays whose objects come in several kinds. The text is a skeleton of the minified document with holes, followed by one block per hole. A block is a table block, which uses the table-v1 header and record rules, or a bucket block, which splits a heterogeneous array into one table block per discriminator value. A table block may hold a `json` column whose cells refer to sub-blocks, so a string that holds JSON is encoded as a table too. The encoder is `deslag_core::table::v2::encode`. The decoder is `deslag_core::table::v2::decode`. The transform identity is `json-table`, transform version 4, format version 2.

For every eligible input `x`, `decode(encode(x))` equals the minified form of `x` byte for byte, as for table-v1. Every byte of the wrapper outside the encoded arrays stays in the skeleton unchanged.

### Example

```text
table-v2 {"page":1,"results":@0,"total":1000}
@0 [3]{id:int,name:str}
1,alpha
2,beta
3,gamma
```

This decodes to:

```json
{"page":1,"results":[{"id":1,"name":"alpha"},{"id":2,"name":"beta"},{"id":3,"name":"gamma"}],"total":1000}
```

### Text grammar

```text
text     = "table-v2 " skeleton 1*( LF label " " block )
skeleton = minified JSON with each encoded array replaced by a hole
hole     = label
label    = "@" index
label-path = label *( "." index )
index    = "0" / %x31-39 *DIGIT
block    = table / buckets
table    = "[" count "]{" v2-column *( "," v2-column ) "}" *( LF record ) *( LF label-path "." index " " sub )
v2-column = path ":" v2-type
v2-type  = type / "json"
buckets  = "[" count "] by " name LF order 1*( LF label-path "." index " " table )
order    = index *( "," index )
sub      = block / "= " skeleton 1*( LF label-path "." index " " block )
```

The `*( LF label-path "." index " " sub )` part of `table`, the `json` type and the `sub` rule apply only to tables with a `json` column. Sub-tables below specifies them. A `label-path` is the label of the enclosing block: `@n` for a top-level block, and a longer path such as `@0.1`, `@0.0.2` or `@0.0.0.0.0` for a bucket table, a sub-block or a hole block of a skeleton block.

- The first line is `table-v2`, one space and the skeleton. Minified JSON holds no raw line break, so the skeleton is exactly the rest of the first line.
- A hole is a label outside any JSON string in the skeleton. A `@` inside a string is text. A `@` cannot occur outside a string in valid JSON, so holes are unambiguous.
- Holes are numbered `@0`, `@1`, `@2` and so on, in skeleton order. Each index occurs once. There is at least one hole.
- Blocks follow in hole order. Each block starts on a new line with its label and one space.
- `count`, `path` and `type` are the table-v1 header grammar rules. A table-v2 column is a `v2-column`: its `v2-type` is a table-v1 `type` or `json`. A `record` is one table-v1 record, as defined in table-v1 Rows.
- A `table` block is a table-v1 text without the `table-v1 ` prefix: the header `[count]{columns}` with the table-v1 `count` rule and the `v2-column` rule, then exactly `count` records, each after a `\n`, with the table-v1 row, type and cell rules. A table block without a `json` column ends after exactly `count` records. A table block with a `json` column ends after its last sub-block, or after its `count` records when it has no reference cell. Any record line, bare or quoted, may start with `@`, so a decoder finds the end of the records by counting them.
- The text ends after the last line of the last block, including its bucket tables and sub-blocks. That line is a record. There is no trailing line break unless that record is empty.
- A block whose header line has `] by ` right after its count is a bucket block. Bucket blocks are specified in Bucket blocks below.

### Bucket blocks

A bucket block holds a heterogeneous array of objects: one table block per value of a string field, the discriminator, and an order line that restores the original row order.

```text
table-v2 @0
@0 [5] by type
0,1,0,2,1
@0.0 [2]{type:str,page.path:str}
view,/a
view,/b
@0.1 [2]{type:str,err.code:int}
error,500
error,404
@0.2 [1]{type:str,at.x:num}
click,1.5
```

This decodes to:

```json
[{"type":"view","page":{"path":"/a"}},{"type":"error","err":{"code":500}},{"type":"view","page":{"path":"/b"}},{"type":"click","at":{"x":1.5}},{"type":"error","err":{"code":404}}]
```

- The header is `[count] by name`. `count` is the number of rows, at least 1. `name` is the discriminator: a non-empty table-v1 `name`, so it holds no `.`.
- The order line follows the header. It holds `count` bucket indexes joined by `,`, one per row in original order. Each index has no leading zeros.
- Buckets are numbered in order of their first row: each index is at most one more than the largest index before it, and the first is `0`. So every bucket has at least one row.
- One table block follows per bucket, in bucket order. Bucket `k` of the bucket block labelled `L` starts on a new line with `L.k` and one space. `L` is `@n` for a top-level block, so its bucket tables are `@n.k`. Its `count` equals the number of times `k` occurs in the order line. Its columns include the discriminator `name` as a column path.
- The decoder takes row `i` from bucket `order[i]`, in each bucket's record order, and joins the rows into one array.

The decoder returns no value for a bucket block with any of these:

- a `count` of 0 or with a leading zero, or a missing or empty `name`, or one that is not a table-v1 name,
- an order line with a wrong number of indexes, an index that is empty, padded, written with a leading zero or out of range,
- a bucket table whose row count does not match the order line, whose header lacks the discriminator column, or that table-v1 would reject,
- a missing, extra or misnumbered bucket label.

The decoder does not compare discriminator values. Encoder output has one distinct value per bucket, but a text whose rows disagree still decodes. A bucket block may have one bucket. A bucket table is always a table block, never a bucket block. A bucket block can itself be a sub-block or the hole block of a skeleton block, so `L` can have several segments: a bucket block labelled `@0.0` has the bucket tables `@0.0.0`, `@0.0.1` and so on. A bucket table may hold a `json` column, and its sub-blocks follow it as Sub-tables describes.

The encoder builds a bucket block for an array with at least two and at most `max_rows` items, every item an object. It picks the discriminator deterministically:

1. The candidates are the keys of the first item, in key order, that hold a string in every item. Empty keys and keys that are not table-v1 names are skipped. Only the first 8 candidates are tried, so encoding stays linear in the input.
2. A candidate applies when its values form at least two buckets, fewer buckets than items, and every bucket passes every row of the table-v1 eligibility table except the gain row, with a one-row minimum. Rows are grouped by their raw string lexeme.
3. Among applicable candidates the shortest bucket block wins. A tie goes to the earlier candidate.

A bucket block replaces a table block only when it is strictly shorter. A homogeneous array therefore keeps its table block.

### Sub-tables

A string value whose text is itself minified JSON, an array of objects or an object that holds such arrays, can be encoded as a sub-block. The cell refers to the sub-block, and the decoder restores the original string lexeme byte for byte.

```text
table-v2 @0
@0 [2]{id:int,data:json}
1,@0
2,"@1"
@0.0 [2]{k:int,v:str}
0,a
1,b
```

This decodes to:

```json
[{"id":1,"data":"[{\"k\":0,\"v\":\"a\"},{\"k\":1,\"v\":\"b\"}]"},{"id":2,"data":"@1"}]
```

Column type `json` is valid only in a table-v2 table block, including a bucket table and a sub-block table. table-v1 text with a `json` column is invalid. A `json` column holds JSON strings, like a `str` column, and `null`. Its cells read as follows:

- A bare cell that is `@` followed by one or more ASCII digits is a reference cell.
- Every other cell reads as in a `str` column: a quoted cell is a string, `null` is `null`, the empty cell is a missing field, and other bare text is a string.
- So a string whose text is `@` followed by digits is always quoted in a `json` column. A bare `@` cell in a `str` column stays string text.

Reference cells are numbered `@0`, `@1`, `@2` and so on, per table, in record order and then column order. After the last record of the table come the sub-blocks, one per reference cell, in that order. Sub-block `k` of the table labelled `L` starts on a new line with `L.k` and one space. A table in a bucket block is labelled `@n.b`, so its sub-blocks are labelled `@n.b.k`. Every label is a `label-path` from Text grammar, so a label grows by one segment per sub-block, bucket table or skeleton hole.

A sub-block is one of these:

- A table block or a bucket block. It decodes to a JSON array.
- A skeleton block: `= `, a skeleton line, then one table or bucket block per hole. The skeleton follows the table-v2 skeleton rules. Its holes are numbered `@0`, `@1` and so on, and the block of hole `h` in the skeleton block labelled `L` starts on a new line with `L.h` and one space. A hole block is never a skeleton block.

The decoder decodes the sub-block to JSON text `t`, which must be valid minified JSON. The cell's value is `t` written as a JSON string with the canonical string escape from table-v1 Lexeme preservation.

Nesting is bounded. A top-level block, its bucket tables and the blocks of top-level holes have level 0. A sub-block of a table at level `d` has level `d + 1`. A skeleton block's hole blocks share its level. The maximum level is 4. Each level escapes the decoded text once more, which at most doubles its length. So nested content decodes to at most 2^4 = 16 times the size it would have at level 0.

The decoder returns no value for a sub-table with any of these:

- a `json` column in table-v1 text,
- a reference cell that is out of order, repeated, or written with a leading zero,
- a missing, extra or misnumbered sub-block label,
- a sub-block at a level above 4,
- a skeleton block with no hole, or with a hole whose block is a skeleton block,
- a sub-block that table-v1, Bucket blocks or the table-v2 Decoder rules would reject, or whose decoded text is not valid minified JSON.

The decoder accepts a `json` column with no reference cell, though the encoder never emits one.

The encoder applies sub-tables after it has chosen each array's table or bucket block. For each string cell of a `str` column, in record order and then column order, the cell becomes a reference cell when all of these hold:

1. The table's level is below 4.
2. The string's decoded text starts with `[` or `{`, and the JSON minifier, run with `max_depth`, accepts it and returns it unchanged. The original lexeme is canonical, because table-v1 accepts only canonical string lexemes. So the decoder restores the lexeme exactly.
3. The text has a sub-block. An array has the table or bucket block that a top-level hole would get, with `max_rows`. An object has a skeleton block when at least one of its member arrays becomes a hole under the Encoder rules below. Sub-tables apply to the sub-block recursively.
4. The reference cell, its label line and its sub-block are strictly shorter than the string cell: `reference length + label length + 2 + sub-block length` is less than the cell's length.

A column with at least one reference cell becomes `json`. Its other strings with reference syntax are then quoted. The rewritten block replaces the block only when it is strictly shorter, so sub-tables never make a block longer. Each step is linear in its text, and the level bound makes the whole encoder linear in the input.

### Decoder

The decoder replaces each hole with the JSON array that its block decodes to, exactly as table-v1 decodes a block. The result must be valid minified JSON: the JSON minifier, run with depth limit `MAX_JSON_DEPTH_CEILING`, must accept it and return it unchanged. Otherwise the text is invalid.

The decoder returns no value for any of these:

- no hole, or a hole that is out of order, repeated, or written with a leading zero,
- a missing, extra or misnumbered block,
- a block that table-v1 would reject, or a record count that does not match,
- a bucket block that Bucket blocks rejects,
- a sub-table that Sub-tables rejects,
- text after the last block,
- a substituted result that is not valid minified JSON, for example a hole in a key position, whitespace in the skeleton, or a duplicate key.

`table::v2::decode` also reads table-v1 text: text that does not start with `table-v2` goes to the table-v1 decoder unchanged. `table::decode` reads table-v1 only.

Encoder output is canonical. The decoder also accepts some non-canonical texts that the encoder does not emit. A hole may stand anywhere a JSON value can stand, for example a whole skeleton such as `table-v2 @0`, or a hole inside an array or a nested object. A table block in a hole may have zero rows; a bucket table may not. Such texts decode, but encoding the result does not reproduce them.

Decoding is linear in the text length. Sub-tables multiply the decoded size of nested content by at most 16, as Sub-tables describes.

### Encoder

- A bare top-level array keeps its table-v1 encoding unless a table-v2 block is strictly shorter. The candidates are the table-v1 table with sub-tables applied, when that changes it, and the bucket block with sub-tables applied. The shorter candidate wins, the table on a tie. Then the text is `table-v2 @0` with that block labelled `@0`, and it must pass the gain check. Otherwise `table::v2::encode` returns exactly `table::encode` output and reasons for input whose first non-whitespace byte is `[`, except that a candidate that fails only the gain check reports `insufficient_gain`. table-v1 text stays stable for existing consumers.
- For a top-level object, the encoder checks each member value that is an array, in member order. A member array has a table block when it passes every row of the table-v1 eligibility table except the gain row, with `max_rows` per array. It has a bucket block under the rules in Bucket blocks. When it has both, the encoder uses the shorter one, and the table block on a tie. It then applies sub-tables to that block. The array becomes a hole when it has a block and `hole length + label length + 2 + block length` is strictly less than the array's minified byte length. For a top-level hole the label is the hole text, so this is `2 * hole length + 2 + block length`. Otherwise the array stays in the skeleton unchanged.
- Arrays below the top-level object's members stay in the skeleton.
- The whole document must pass the JSON minifier first, so a duplicate or escaped key anywhere, or nesting deeper than `max_depth`, skips the whole input with `duplicate_key`, `escaped_key` or `depth_limit`.

| Case | Reason |
| --- | --- |
| The input is not JSON, or is a bare scalar | `unsupported_format` |
| The top-level object has no member array that becomes a hole | `unsupported_format` |
| The gain check on the whole text fails | `insufficient_gain` |

The gain check is the table-v1 gain check, applied to the whole table-v2 text against the minified document.

Known limit: per-hole selection uses only the strict byte comparison above. It applies no `min_savings_permille` threshold and no token check. The gain check applies only to the whole table-v2 text.

### Integration

`compress` calls `table::v2::encode` when `json_table` is true, and validates the candidate with `table::v2::decode`. The report's format version is 2 for table-v2 text and 1 for table-v1 text. The transform version is 4 for both. All other rules in the table-v1 Pipeline and configuration section apply unchanged.

### Reserved extensions

A table-v2 block whose header does not start with `[` is invalid in this version, except a sub-block that starts with `= `. A header that starts with `[` and continues after its count with anything other than `]{` or `] by ` is also invalid. A column type other than `int`, `num`, `str`, `bool` and `json` is invalid. A bare `@` cell in a `str` column stays string text in every revision.
