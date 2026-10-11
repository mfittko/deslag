# Benchmark baseline

PLAN §13.2 budgets remain provisional. This file records measurements and sets no release thresholds.

## Metadata

| Field | Value |
| --- | --- |
| Date (UTC) | 2026-10-11T18:07:47Z |
| Commit | b25b7639ce8a2bc5d81210e82a051023d5aa92db |
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
| no-op (no flag) | 1.277625ms | 1.585875ms | 1.777541ms |
| --minify-json | 1.539875ms | 1.621167ms | 1.763833ms |

### deslag-core minify_json (concurrency 1, warmup 10)

| input | bytes | changed | samples | p50 | p95 | p99 | MiB/s at p50 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| repetitive | 2131 | yes | 2000 | 5.708µs | 6.333µs | 6.458µs | 356 |
| incompressible | 2071 | no | 2000 | 1.583µs | 1.625µs | 1.708µs | 1248 |
| repetitive | 16468 | yes | 2000 | 30.417µs | 42.167µs | 42.875µs | 516 |
| incompressible | 16421 | no | 2000 | 7.375µs | 8.875µs | 9.5µs | 2123 |
| repetitive | 65627 | yes | 1022 | 96.083µs | 102.333µs | 114.75µs | 651 |
| incompressible | 65561 | no | 1023 | 27.875µs | 29.334µs | 32.416µs | 2243 |
| repetitive | 262193 | yes | 255 | 384.708µs | 402.708µs | 412.709µs | 650 |
| incompressible | 262156 | no | 255 | 109.042µs | 115.5µs | 127.875µs | 2293 |
| repetitive | 1048628 | yes | 100 | 1.525583ms | 1.582875ms | 1.589666ms | 656 |
| incompressible | 1048606 | no | 100 | 438.625µs | 459.875µs | 468.541µs | 2280 |

### deslag-proxy request latency (273190 byte request, concurrency 1, warmup 20, samples 500)

| path | upstream bytes | p50 | p95 | p99 |
| --- | --- | --- | --- | --- |
| direct to mock | 273190 | 89.5µs | 112.584µs | 122.708µs |
| proxy, transparent | 273190 | 104.25µs | 117.125µs | 122.709µs |
| proxy, --minify-json | 244704 | 1.181ms | 1.2335ms | 1.27775ms |

