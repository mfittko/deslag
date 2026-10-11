# Benchmark baseline

PLAN §13.2 budgets remain provisional. This file records measurements and sets no release thresholds.

## Metadata

| Field | Value |
| --- | --- |
| Date (UTC) | 2026-10-11T17:21:33Z |
| Commit | a47ce5f8a3673dd223f56080353ddc30bad1b33f |
| Uncommitted changes (tracked or untracked) | no |
| CPU | Apple M5 Max |
| Logical CPUs | 18 |
| OS | Darwin 25.6.0 Darwin Kernel Version 25.6.0: Sat Jul 11 15:27:04 PDT 2026; root:xnu-12377.161.13~4/RELEASE_ARM64_T6050 arm64 |
| cargo | cargo 1.96.0 (30a34c682 2026-05-25) (Homebrew) |
| Profile | bench (release), default features |

`rustc -Vv`:

```
rustc 1.96.0 (ac68faa20 2026-05-25) (Homebrew)
binary: rustc
commit-hash: ac68faa20c58cbccd01ee7208bf3b6e93a7d7f96
commit-date: 2026-05-25
host: aarch64-apple-darwin
release: 1.96.0
LLVM version: 22.1.6
```

## Results

### deslag hook claude, whole process (78633 byte event, concurrency 1, warmup 10, samples 200)

| mode | p50 | p95 | p99 |
| --- | --- | --- | --- |
| no-op (no flag) | 1.267666ms | 1.688875ms | 1.935917ms |
| --minify-json | 1.548167ms | 1.710041ms | 1.814ms |

### deslag-core minify_json (concurrency 1, warmup 10)

| input | bytes | changed | samples | p50 | p95 | p99 | MiB/s at p50 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| repetitive | 2131 | yes | 2000 | 7.542µs | 8.791µs | 9.042µs | 269 |
| incompressible | 2071 | no | 2000 | 1.792µs | 2.166µs | 2.25µs | 1102 |
| repetitive | 16468 | yes | 2000 | 30.625µs | 43.667µs | 44.709µs | 513 |
| incompressible | 16421 | no | 2000 | 7.209µs | 7.917µs | 8.042µs | 2172 |
| repetitive | 65627 | yes | 1022 | 96.792µs | 105.917µs | 114.5µs | 647 |
| incompressible | 65561 | no | 1023 | 27.375µs | 30.541µs | 37.625µs | 2284 |
| repetitive | 262193 | yes | 255 | 385.791µs | 412.5µs | 420.583µs | 648 |
| incompressible | 262156 | no | 255 | 109.792µs | 119.875µs | 125.5µs | 2277 |
| repetitive | 1048628 | yes | 100 | 1.596417ms | 1.647458ms | 1.664125ms | 626 |
| incompressible | 1048606 | no | 100 | 438.75µs | 463.125µs | 477.708µs | 2279 |

### deslag-proxy request latency (273190 byte request, concurrency 1, warmup 20, samples 500)

| path | upstream bytes | p50 | p95 | p99 |
| --- | --- | --- | --- | --- |
| direct to mock | 273190 | 84.666µs | 103.5µs | 111.833µs |
| proxy, transparent | 273190 | 107.291µs | 122.584µs | 130.334µs |
| proxy, --minify-json | 244704 | 1.198875ms | 1.312459ms | 1.3505ms |

