### Changed

- The release workflow publishes through `scripts/release/publish.sh`, with tests for re-runs on complete, empty and partial releases (#45)

### Fixed

- `bump.sh` refreshes `fuzz/Cargo.lock`, and `check-versions.sh` fails when it pins another `deslag-core` version (#67)
