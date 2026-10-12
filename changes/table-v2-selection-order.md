### Changed

- The `table-v2` encoder now picks the table or bucket block for a hole or a sub-block array after sub-tables apply to both candidates, as it already did for a bare array, so one array gets the same block at every site. The `json-table` transform version is now 5 (#65)
