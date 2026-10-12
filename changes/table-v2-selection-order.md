### Changed

- The `table-v2` encoder now picks the table or bucket block for a hole or a sub-block array after sub-tables apply to both candidates, as it already did for a bare array, so one array gets the same block at every site. Selection compares bytes only, so with a token counter a bare array whose table-v1 text fails only the token check now passes through where a longer bucket block used to win. The `json-table` transform version is now 5 (#65)
