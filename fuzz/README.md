# Fuzz targets

cargo-fuzz crate, outside the root workspace. Corpus and artifacts stay untracked.

Run a target on a stable toolchain:

```sh
cd fuzz
RUSTC_BOOTSTRAP=1 cargo fuzz run json_minify -s none -- -max_total_time=60
```

On nightly, `cargo fuzz run <target> -s none` works without `RUSTC_BOOTSTRAP`.

List the targets with `cargo fuzz list`. The `[[bin]]` entries in `Cargo.toml` define them.
Recorded runs are in `docs/benchmarks/fuzz.md`.

## Adding a transform

1. Add `fuzz_targets/<target>.rs` that checks the transform round trip or its invariant. The target name is the transform ID with `-` replaced by `_`, for example `exact_line_runs.rs` for `exact-line-runs`.
2. Add one `[[bin]]` entry with the same target name to `Cargo.toml`.
3. Add a row for the target to `docs/benchmarks/fuzz.md` after its first recorded run.
4. Enable the transform in the `compress` target and keep its invariants there.
