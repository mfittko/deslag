# Benchmark baseline

PLAN §13.2 budgets remain provisional. This file records measurements only; release thresholds are not frozen here.

## Metadata

| Field | Value |
| --- | --- |
| Date (UTC) | 2026-10-11T17:12:53Z |
| Commit | 8ab0503f97c5f28482837af70e785873b08b9bd8 |
| Uncommitted tracked changes | no |
| CPU | Apple M5 Max |
| Logical CPUs | 18 |
| OS | Darwin st13521495 25.6.0 Darwin Kernel Version 25.6.0: Sat Jul 11 15:27:04 PDT 2026; root:xnu-12377.161.13~4/RELEASE_ARM64_T6050 arm64 |
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
| no-op (no flag) | 1.386041ms | 1.5925ms | 1.717875ms |
| --minify-json | 1.569292ms | 1.698208ms | 1.8895ms |

### deslag-core minify_json (concurrency 1, warmup 10)

| input | bytes | changed | samples | p50 | p95 | p99 | MiB/s at p50 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| repetitive | 2131 | yes | 2000 | 7.375µs | 8.792µs | 8.917µs | 276 |
| incompressible | 2071 | no | 2000 | 1.417µs | 1.583µs | 1.916µs | 1394 |
| repetitive | 16468 | yes | 2000 | 32.416µs | 43.5µs | 46.541µs | 484 |
| incompressible | 16421 | no | 2000 | 7.917µs | 9µs | 13.375µs | 1978 |
| repetitive | 65627 | yes | 1022 | 99.833µs | 104.458µs | 112.375µs | 627 |
| incompressible | 65561 | no | 1023 | 29.458µs | 30.625µs | 34.416µs | 2122 |
| repetitive | 262193 | yes | 255 | 405.084µs | 427.042µs | 438.625µs | 617 |
| incompressible | 262156 | no | 255 | 113.625µs | 122.625µs | 133.541µs | 2200 |
| repetitive | 1048628 | yes | 100 | 1.599083ms | 1.642833ms | 1.673ms | 625 |
| incompressible | 1048606 | no | 100 | 447.917µs | 472.25µs | 479.333µs | 2233 |

### deslag-proxy request latency (273190 byte request, concurrency 1, warmup 20, samples 500)

| path | upstream bytes | p50 | p95 | p99 |
| --- | --- | --- | --- | --- |
| direct to mock | 273190 | 79.625µs | 159.375µs | 196.792µs |
| proxy, transparent | 273190 | 107.875µs | 128.125µs | 138.584µs |
| proxy, --minify-json | 244704 | 1.183584ms | 1.250667ms | 1.343792ms |

