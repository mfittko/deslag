# Compression formats

Formats that deslag transforms emit. Each format is versioned. A decoder for a
version accepts every output that version's encoder can produce.

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
  and `exact_line_runs` on, all input goes to this codec.
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
  value, a missing `\n`, or the reserved prefix without the escape.
- The output bound applies as in `lines-v1`.

### Encoder

At each unit, in input order:

1. Count the identical consecutive copies. When there are at least 2 and the
   `lines-v1` run rule pays, write the unit and a run directive.
2. Otherwise grow a group greedily from this unit as the base. A following
   unit joins when it ends with `\n`, has the same gaps byte for byte and the
   same token count as the base, and each of its tokens equals the base token
   or both tokens are variable-class. A constant token that differs ends the
   group, so lines that differ in an error message never share a template.
3. The variable indices are the indices where any row differs from the base.
   The group is written only when the directive and the rows are shorter than
   the rows' literal bytes, escapes included. Otherwise the base is written
   literally, and no group starts at the other units of that group. Run
   directives still apply to them.

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
