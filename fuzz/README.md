# Fuzz targets

cargo-fuzz crate, outside the root workspace. Corpus and artifacts stay untracked.

Run a target on a stable toolchain:

```sh
cd fuzz
RUSTC_BOOTSTRAP=1 cargo fuzz run json_minify -s none -- -max_total_time=60
```

On nightly, `cargo fuzz run <target> -s none` works without `RUSTC_BOOTSTRAP`.

Targets: `json_minify`, `json_table`, `exact_line_runs`, `line_templates`, `compress`.
Recorded runs are in `docs/benchmarks/fuzz.md`.

## Adding a transform

1. Add `fuzz_targets/<transform_id>.rs` that checks the transform round trip or its invariant.
2. Add one `[[bin]]` entry to `Cargo.toml`.
3. Enable the transform in the `compress` target and keep its invariants there.
