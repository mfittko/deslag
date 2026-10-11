# Benchmark baseline

PLAN §13.2 budgets remain provisional. This file records measurements and sets no release thresholds.

## Metadata

| Field | Value |
| --- | --- |
| Date (UTC) | 2026-10-11T18:32:45Z |
| Commit | 0e47f5cb38a646c76cd461aad9dc39bdb7be36e8 |
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
| no-op (no flag) | 1.241875ms | 1.34175ms | 1.372375ms |
| --minify-json | 1.524833ms | 1.648125ms | 1.763209ms |

### deslag-core minify_json (concurrency 1, warmup 10)

| input | bytes | changed | samples | p50 | p95 | p99 | MiB/s at p50 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| repetitive | 2131 | yes | 2000 | 3.417µs | 3.625µs | 4.458µs | 595 |
| incompressible | 2071 | no | 2000 | 875ns | 917ns | 917ns | 2257 |
| repetitive | 16468 | yes | 2000 | 24.625µs | 26.208µs | 29.209µs | 638 |
| incompressible | 16421 | no | 2000 | 6.875µs | 7.583µs | 9.25µs | 2278 |
| repetitive | 65627 | yes | 1022 | 103.75µs | 109.667µs | 133.333µs | 603 |
| incompressible | 65561 | no | 1023 | 28µs | 30.375µs | 35.917µs | 2233 |
| repetitive | 262193 | yes | 255 | 390.958µs | 406.084µs | 415.583µs | 640 |
| incompressible | 262156 | no | 255 | 109.042µs | 119.5µs | 126.208µs | 2293 |
| repetitive | 1048628 | yes | 100 | 1.534166ms | 1.581583ms | 1.6095ms | 652 |
| incompressible | 1048606 | no | 100 | 440.542µs | 459.792µs | 465.833µs | 2270 |

### deslag-proxy request latency (273190 byte request, concurrency 1, warmup 20, samples 500)

| path | upstream bytes | p50 | p95 | p99 |
| --- | --- | --- | --- | --- |
| direct to mock | 273190 | 91µs | 104.292µs | 110.875µs |
| proxy, transparent | 273190 | 100.5µs | 118.125µs | 138.917µs |
| proxy, --minify-json | 244704 | 1.169875ms | 1.22025ms | 1.264458ms |

