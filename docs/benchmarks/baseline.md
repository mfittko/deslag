# Benchmark baseline

PLAN §13.2 budgets remain provisional. This file records measurements and sets no release thresholds.

## Metadata

| Field | Value |
| --- | --- |
| Date (UTC) | 2026-10-12T01:13:43Z |
| Commit | 56b821d0137c7f34f940cc07d251ce74304a8e47 |
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
| no-op (no flag) | 1.513959ms | 1.728959ms | 1.887417ms |
| --minify-json | 1.849833ms | 2.1275ms | 2.202292ms |

### deslag-core stages (concurrency 1, warmup 10)

Corpus `generated-v1`. Tokenizer: method `estimate`, `deslag-estimate-v1`. Samples per row: 64 MiB divided by the input size, clamped to 100..=2000. Allocations, bytes and peak heap come from one call after warmup. A growing realloc counts as one allocation of the added bytes.

| stage | input | bytes | output (bytes; tokens for token estimate) | samples | p50 | p95 | p99 | MiB/s at p50 | allocs/call | alloc bytes/call | peak heap bytes/call |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| minify scan | repetitive | 2074 | 1662 | 2000 | 3.583µs | 3.917µs | 4.041µs | 552 | 42 | 5190 | 2150 |
| minify scan | incompressible | 2071 | 2071 | 2000 | 1.042µs | 1.083µs | 1.25µs | 1895 | 1 | 2071 | 2071 |
| minify scan | repetitive | 16425 | 13283 | 2000 | 26.792µs | 28.292µs | 29.458µs | 585 | 315 | 40289 | 16501 |
| minify scan | incompressible | 16421 | 16421 | 2000 | 7.625µs | 8.959µs | 9.084µs | 2054 | 1 | 16421 | 16421 |
| minify scan | repetitive | 65543 | 53221 | 1023 | 104.584µs | 107.125µs | 109.292µs | 598 | 1233 | 159175 | 65619 |
| minify scan | incompressible | 65561 | 65561 | 1023 | 30.334µs | 31.209µs | 33.125µs | 2061 | 1 | 65561 | 65561 |
| minify scan | repetitive | 262168 | 214096 | 255 | 411.25µs | 426.792µs | 443.125µs | 608 | 4808 | 627500 | 262244 |
| minify scan | incompressible | 262156 | 262156 | 255 | 121.042µs | 126.208µs | 129.375µs | 2065 | 1 | 262156 | 262156 |
| minify scan | repetitive | 1048585 | 860723 | 100 | 1.626958ms | 1.723375ms | 1.733ms | 615 | 18787 | 2476321 | 1048661 |
| minify scan | incompressible | 1048606 | 1048606 | 100 | 483µs | 492.459µs | 497.666µs | 2070 | 1 | 1048606 | 1048606 |
| table-v1 encode | repetitive | 2074 | 678 | 2000 | 20.166µs | 21.542µs | 22.375µs | 98 | 380 | 28755 | 11460 |
| table-v1 encode | incompressible | 2099 | 1400 | 2000 | 21.375µs | 22µs | 22.833µs | 94 | 311 | 25136 | 9869 |
| table-v1 encode | repetitive | 16425 | 5475 | 2000 | 144.917µs | 152.625µs | 198.167µs | 108 | 2576 | 215861 | 87969 |
| table-v1 encode | incompressible | 16457 | 10831 | 2000 | 149.834µs | 154.708µs | 159µs | 105 | 2083 | 188828 | 74915 |
| table-v1 encode | repetitive | 65543 | 22464 | 1023 | 559.042µs | 572.916µs | 593.459µs | 112 | 9928 | 850685 | 348184 |
| table-v1 encode | incompressible | 65575 | 43177 | 1023 | 600.25µs | 919.666µs | 1.182417ms | 104 | 8081 | 747660 | 297041 |
| table-v1 encode | repetitive | 262168 | 93964 | 255 | 2.821417ms | 3.835208ms | 13.387458ms | 89 | 38536 | 3355811 | 1378648 |
| table-v1 encode | incompressible | 262169 | 173384 | 255 | 3.077917ms | 7.486791ms | 12.976208ms | 81 | 31799 | 2970932 | 1181267 |
| table-v1 encode | repetitive | 1048585 | 391117 | 100 | 8.770333ms | 8.966834ms | 9.044333ms | 114 | 150376 | 13261469 | 5465231 |
| table-v1 encode | incompressible | 1048637 | 695421 | 100 | 9.245334ms | 9.740667ms | 9.824666ms | 108 | 126247 | 11844524 | 4711223 |
| exact-line-runs | repetitive | 2160 | 218 | 2000 | 1.875µs | 2.041µs | 2.125µs | 1099 | 23 | 3916 | 3816 |
| exact-line-runs | incompressible | 2090 | 2090 | 2000 | 791ns | 792ns | 834ns | 2520 | 7 | 2336 | 2336 |
| exact-line-runs | repetitive | 16416 | 1538 | 2000 | 11.541µs | 12µs | 12.375µs | 1357 | 95 | 31288 | 30528 |
| exact-line-runs | incompressible | 16390 | 16390 | 2000 | 4.625µs | 5µs | 5.541µs | 3380 | 10 | 18688 | 18688 |
| exact-line-runs | repetitive | 65664 | 6098 | 1022 | 44.708µs | 46.209µs | 47.333µs | 1401 | 327 | 125152 | 122112 |
| exact-line-runs | incompressible | 65560 | 65560 | 1023 | 17.916µs | 18.958µs | 30.417µs | 3490 | 12 | 74752 | 74752 |
| exact-line-runs | repetitive | 262224 | 24298 | 255 | 174.5µs | 182.708µs | 191.375µs | 1433 | 1241 | 500588 | 488448 |
| exact-line-runs | incompressible | 262185 | 262185 | 255 | 65.375µs | 67.417µs | 68.292µs | 3825 | 14 | 299008 | 299008 |
| exact-line-runs | repetitive | 1048896 | 97138 | 100 | 676.917µs | 694.541µs | 716.208µs | 1478 | 4887 | 2002352 | 1953792 |
| exact-line-runs | incompressible | 1048630 | 1048630 | 100 | 263.583µs | 270.625µs | 283.709µs | 3794 | 16 | 1196032 | 1196032 |
| line-templates | repetitive | 2086 | 1341 | 2000 | 13.125µs | 13.625µs | 14.125µs | 152 | 348 | 30132 | 14696 |
| line-templates | incompressible | 2090 | 2090 | 2000 | 8.417µs | 8.75µs | 9.291µs | 237 | 279 | 18832 | 3944 |
| line-templates | repetitive | 16433 | 10169 | 2000 | 93.375µs | 112.334µs | 121.625µs | 168 | 2520 | 236856 | 115236 |
| line-templates | incompressible | 16390 | 16352 | 2000 | 74.875µs | 97.5µs | 101.084µs | 209 | 2331 | 181592 | 46848 |
| line-templates | repetitive | 65567 | 40398 | 1023 | 368.333µs | 384.625µs | 458.5µs | 170 | 9926 | 929582 | 444217 |
| line-templates | incompressible | 65560 | 65300 | 1023 | 302.666µs | 324.166µs | 389µs | 207 | 9562 | 740490 | 187660 |
| line-templates | repetitive | 262178 | 161362 | 255 | 1.411667ms | 1.470791ms | 1.486459ms | 177 | 39535 | 3716560 | 1775805 |
| line-templates | incompressible | 262185 | 260833 | 255 | 1.193292ms | 1.213417ms | 1.25225ms | 210 | 37721 | 2954081 | 749836 |
| line-templates | repetitive | 1048630 | 645227 | 100 | 5.757291ms | 5.845833ms | 5.871ms | 174 | 157947 | 14864522 | 7102166 |
| line-templates | incompressible | 1048630 | 1043353 | 100 | 4.840167ms | 4.954125ms | 5.072208ms | 207 | 150016 | 11812511 | 2998540 |
| token estimate | repetitive | 2074 | 824 | 2000 | 3.333µs | 3.708µs | 4.208µs | 593 | 0 | 0 | 0 |
| token estimate | incompressible | 2071 | 833 | 2000 | 2.791µs | 2.916µs | 3.042µs | 708 | 0 | 0 | 0 |
| token estimate | repetitive | 16425 | 6284 | 2000 | 28.083µs | 29.375µs | 30.833µs | 558 | 0 | 0 | 0 |
| token estimate | incompressible | 16421 | 6738 | 2000 | 23.125µs | 24.833µs | 41.958µs | 677 | 0 | 0 | 0 |
| token estimate | repetitive | 65543 | 25108 | 1023 | 109.708µs | 112.75µs | 117.209µs | 570 | 0 | 0 | 0 |
| token estimate | incompressible | 65561 | 27124 | 1023 | 141.791µs | 144.417µs | 149.042µs | 441 | 0 | 0 | 0 |
| token estimate | repetitive | 262168 | 103758 | 255 | 437.666µs | 450.458µs | 455.459µs | 571 | 0 | 0 | 0 |
| token estimate | incompressible | 262156 | 108384 | 255 | 722.75µs | 763.75µs | 841.167µs | 346 | 0 | 0 | 0 |
| token estimate | repetitive | 1048585 | 411296 | 100 | 1.712334ms | 1.770042ms | 1.774958ms | 584 | 0 | 0 | 0 |
| token estimate | incompressible | 1048606 | 433420 | 100 | 3.016958ms | 3.11175ms | 3.187625ms | 331 | 0 | 0 | 0 |
| full compress + gain | repetitive | 2074 | 678 | 2000 | 28µs | 29.208µs | 30.125µs | 71 | 380 | 28755 | 11460 |
| full compress + gain | incompressible | 2071 | 2071 | 2000 | 7.625µs | 8.375µs | 9.5µs | 259 | 4 | 4182 | 2111 |
| full compress + gain | repetitive | 16425 | 5475 | 2000 | 206.125µs | 212.042µs | 221.125µs | 76 | 2576 | 215861 | 87969 |
| full compress + gain | incompressible | 16421 | 16421 | 2000 | 60.5µs | 61.791µs | 64µs | 259 | 4 | 32882 | 16461 |
| full compress + gain | repetitive | 65543 | 22464 | 1023 | 812.166µs | 832µs | 864.125µs | 77 | 9928 | 850685 | 348184 |
| full compress + gain | incompressible | 65561 | 65561 | 1023 | 336.375µs | 375.25µs | 420.375µs | 186 | 4 | 131162 | 65601 |
| full compress + gain | repetitive | 262168 | 93964 | 255 | 3.20875ms | 3.278584ms | 3.323625ms | 78 | 38536 | 3355811 | 1378648 |
| full compress + gain | incompressible | 262156 | 262156 | 255 | 1.697375ms | 1.769166ms | 1.821959ms | 147 | 4 | 524352 | 262196 |
| full compress + gain | repetitive | 1048585 | 391117 | 100 | 12.780958ms | 13.25ms | 14.436459ms | 78 | 150376 | 13261469 | 5465231 |
| full compress + gain | incompressible | 1048606 | 1048606 | 100 | 7.057167ms | 7.318625ms | 8.05775ms | 142 | 4 | 2097252 | 1048646 |

### deslag-core bounded execution (64 KiB repetitive, full compress, 200 calls per thread)

Default limits: 4 jobs, 10ms budget.

| limits | concurrency | max jobs | completed/s | completed | busy | budget_exceeded | job_panicked |
| --- | --- | --- | --- | --- | --- | --- | --- |
| default | 1 | 4 | 1054 | 200 | 0.0% | 0.0% | 0.0% |
| max jobs = concurrency | 1 | 1 | 1086 | 200 | 0.0% | 0.0% | 0.0% |
| default | 8 | 4 | 1722 | 800 | 50.0% | 0.0% | 0.0% |
| max jobs = concurrency | 8 | 8 | 1683 | 1600 | 0.0% | 0.0% | 0.0% |
| default | 32 | 4 | 1716 | 800 | 87.5% | 0.0% | 0.0% |
| max jobs = concurrency | 32 | 32 | 1929 | 5073 | 17.8% | 2.9% | 0.0% |

### deslag-core minify_json (concurrency 1, warmup 10)

| input | bytes | changed | samples | p50 | p95 | p99 | MiB/s at p50 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| repetitive | 2131 | yes | 2000 | 3.542µs | 3.792µs | 4µs | 574 |
| incompressible | 2071 | no | 2000 | 1.042µs | 1.083µs | 1.084µs | 1895 |
| repetitive | 16468 | yes | 2000 | 26.083µs | 26.917µs | 28µs | 602 |
| incompressible | 16421 | no | 2000 | 7.667µs | 7.792µs | 8.459µs | 2043 |
| repetitive | 65627 | yes | 1022 | 102.834µs | 106.583µs | 110.625µs | 609 |
| incompressible | 65561 | no | 1023 | 30.417µs | 31.375µs | 36.75µs | 2056 |
| repetitive | 262193 | yes | 255 | 407.083µs | 415.084µs | 420.084µs | 614 |
| incompressible | 262156 | no | 255 | 121.25µs | 123.791µs | 131.583µs | 2062 |
| repetitive | 1048628 | yes | 100 | 1.621ms | 1.644083ms | 1.878ms | 617 |
| incompressible | 1048606 | no | 100 | 487.041µs | 591.625µs | 610.167µs | 2053 |

### deslag-proxy request latency (273190 byte request, concurrency 1, warmup 20, samples 500)

| path | upstream bytes | p50 | p95 | p99 |
| --- | --- | --- | --- | --- |
| direct to mock | 273190 | 84.958µs | 102µs | 113.583µs |
| proxy, transparent | 273190 | 98.208µs | 139.375µs | 245.459µs |
| proxy, --minify-json | 244704 | 1.239083ms | 1.305666ms | 1.371542ms |

