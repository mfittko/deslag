### Added

- `deslag-core` adds `BoundedExecutor`: a parallel job limit and a processing budget (defaults 4 jobs, 10 ms). A busy or expired call returns `Rejected` so the caller passes the original through. An expired job keeps its permit until it ends (#15)
