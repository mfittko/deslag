### Added

- `deslag-core` table-v2 splits a heterogeneous array of objects, such as events whose fields differ by a `type` field, into one table per discriminator value plus an order line, and decodes the rows back in their original order. This applies to bare top-level arrays, which keep table-v1 output unless the bucket form is shorter, and to arrays inside a wrapper object. The `json-table` transform version is 3, because encoder output changes (#43)
