# Benchmark baseline

PLAN §13.2 budgets remain provisional. This file records measurements and sets no release thresholds.

## Metadata

| Field | Value |
| --- | --- |
| Date (UTC) | 2026-10-12T00:07:51Z |
| Commit | 8fc523317058a54cb09fe261d69bb8e73f06ff64 |
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
| no-op (no flag) | 1.361583ms | 1.4655ms | 1.639042ms |
| --minify-json | 1.761791ms | 2.294916ms | 2.521166ms |

### deslag-core stages (concurrency 1, warmup 10)

Corpus `generated-v1`. Tokenizer: method `estimate`, `deslag-estimate-v1`. Samples per row: 64 MiB divided by the input size, clamped to 100..=2000. Allocations, bytes and peak heap come from one call after warmup.

| stage | input | bytes | output bytes | samples | p50 | p95 | p99 | MiB/s at p50 | allocs/call | alloc bytes/call | peak heap bytes/call |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| minify scan | repetitive | 2074 | 1662 | 2000 | 5.334µs | 5.792µs | 5.917µs | 371 | 42 | 5190 | 2150 |
| minify scan | incompressible | 2071 | 2071 | 2000 | 1.417µs | 1.459µs | 1.542µs | 1394 | 1 | 2071 | 2071 |
| minify scan | repetitive | 16425 | 13283 | 2000 | 28.625µs | 34.25µs | 37µs | 547 | 315 | 40289 | 16501 |
| minify scan | incompressible | 16421 | 16421 | 2000 | 7.583µs | 7.791µs | 7.959µs | 2065 | 1 | 16421 | 16421 |
| minify scan | repetitive | 65543 | 53221 | 1023 | 104.375µs | 106.917µs | 113.084µs | 599 | 1233 | 159175 | 65619 |
| minify scan | incompressible | 65561 | 65561 | 1023 | 30.167µs | 30.833µs | 36.292µs | 2073 | 1 | 65561 | 65561 |
| minify scan | repetitive | 262168 | 214096 | 255 | 408.125µs | 424.083µs | 432.083µs | 613 | 4808 | 627500 | 262244 |
| minify scan | incompressible | 262156 | 262156 | 255 | 120.291µs | 123.25µs | 128.792µs | 2078 | 1 | 262156 | 262156 |
| minify scan | repetitive | 1048585 | 860723 | 100 | 1.622708ms | 1.712833ms | 1.724ms | 616 | 18787 | 2476321 | 1048661 |
| minify scan | incompressible | 1048606 | 1048606 | 100 | 479.083µs | 493.041µs | 498.208µs | 2087 | 1 | 1048606 | 1048606 |
| table-v1 encode | repetitive | 2074 | 678 | 2000 | 19.958µs | 21.583µs | 22.625µs | 99 | 380 | 28755 | 11460 |
| table-v1 encode | incompressible | 2099 | 1400 | 2000 | 20.875µs | 21.584µs | 22.875µs | 96 | 311 | 25136 | 9869 |
| table-v1 encode | repetitive | 16425 | 5475 | 2000 | 143.292µs | 150.166µs | 161.334µs | 109 | 2576 | 215861 | 87969 |
| table-v1 encode | incompressible | 16457 | 10831 | 2000 | 147.792µs | 156.25µs | 166µs | 106 | 2083 | 188828 | 74915 |
| table-v1 encode | repetitive | 65543 | 22464 | 1023 | 555.083µs | 575.584µs | 587.166µs | 113 | 9928 | 850685 | 348184 |
| table-v1 encode | incompressible | 65575 | 43177 | 1023 | 601.625µs | 622.708µs | 642.209µs | 104 | 8081 | 747660 | 297041 |
| table-v1 encode | repetitive | 262168 | 93964 | 255 | 2.175959ms | 2.260584ms | 2.327875ms | 115 | 38536 | 3355811 | 1378648 |
| table-v1 encode | incompressible | 262169 | 173384 | 255 | 2.356125ms | 2.485ms | 2.51ms | 106 | 31799 | 2970932 | 1181267 |
| table-v1 encode | repetitive | 1048585 | 391117 | 100 | 8.617125ms | 8.908125ms | 9.234167ms | 116 | 150376 | 13261469 | 5465231 |
| table-v1 encode | incompressible | 1048637 | 695421 | 100 | 9.157875ms | 9.5945ms | 9.718959ms | 109 | 126247 | 11844524 | 4711223 |
| exact-line-runs | repetitive | 2160 | 218 | 2000 | 1.917µs | 2.084µs | 2.333µs | 1075 | 23 | 3916 | 3816 |
| exact-line-runs | incompressible | 2090 | 2090 | 2000 | 833ns | 959ns | 1.042µs | 2393 | 7 | 2336 | 2336 |
| exact-line-runs | repetitive | 16416 | 1538 | 2000 | 11.333µs | 12µs | 12.375µs | 1381 | 95 | 31288 | 30528 |
| exact-line-runs | incompressible | 16390 | 16390 | 2000 | 4.417µs | 4.792µs | 5.041µs | 3539 | 10 | 18688 | 18688 |
| exact-line-runs | repetitive | 65664 | 6098 | 1022 | 43.625µs | 46µs | 47.292µs | 1435 | 327 | 125152 | 122112 |
| exact-line-runs | incompressible | 65560 | 65560 | 1023 | 17.416µs | 18.459µs | 19.458µs | 3590 | 12 | 74752 | 74752 |
| exact-line-runs | repetitive | 262224 | 24298 | 255 | 171.792µs | 175.208µs | 179.208µs | 1456 | 1241 | 500588 | 488448 |
| exact-line-runs | incompressible | 262185 | 262185 | 255 | 66.583µs | 69.042µs | 71.375µs | 3755 | 14 | 299008 | 299008 |
| exact-line-runs | repetitive | 1048896 | 97138 | 100 | 669.75µs | 688.959µs | 706.167µs | 1494 | 4887 | 2002352 | 1953792 |
| exact-line-runs | incompressible | 1048630 | 1048630 | 100 | 252.959µs | 266.542µs | 273.416µs | 3953 | 16 | 1196032 | 1196032 |
| token estimate | repetitive | 2074 | 824 | 2000 | 5µs | 5.334µs | 10.208µs | 396 | 0 | 0 | 0 |
| token estimate | incompressible | 2071 | 833 | 2000 | 3.542µs | 3.792µs | 8.792µs | 558 | 0 | 0 | 0 |
| token estimate | repetitive | 16425 | 6284 | 2000 | 38.417µs | 41.542µs | 45.625µs | 408 | 0 | 0 | 0 |
| token estimate | incompressible | 16421 | 6738 | 2000 | 34.375µs | 35.5µs | 40µs | 456 | 0 | 0 | 0 |
| token estimate | repetitive | 65543 | 25108 | 1023 | 156.667µs | 167.542µs | 176.5µs | 399 | 0 | 0 | 0 |
| token estimate | incompressible | 65561 | 27124 | 1023 | 178.875µs | 212.167µs | 234.209µs | 350 | 0 | 0 | 0 |
| token estimate | repetitive | 262168 | 103758 | 255 | 604.083µs | 645.25µs | 652.375µs | 414 | 0 | 0 | 0 |
| token estimate | incompressible | 262156 | 108384 | 255 | 936.125µs | 990.458µs | 1.016709ms | 267 | 0 | 0 | 0 |
| token estimate | repetitive | 1048585 | 411296 | 100 | 2.39475ms | 2.500125ms | 2.514208ms | 418 | 0 | 0 | 0 |
| token estimate | incompressible | 1048606 | 433420 | 100 | 3.79625ms | 3.910084ms | 3.979375ms | 263 | 0 | 0 | 0 |
| full compress + gain | repetitive | 2074 | 678 | 2000 | 31.166µs | 32.375µs | 35.75µs | 63 | 380 | 28755 | 11460 |
| full compress + gain | incompressible | 2071 | 2071 | 2000 | 9.709µs | 10.166µs | 10.5µs | 203 | 4 | 4182 | 2111 |
| full compress + gain | repetitive | 16425 | 5475 | 2000 | 228.083µs | 237µs | 249.75µs | 69 | 2576 | 215861 | 87969 |
| full compress + gain | incompressible | 16421 | 16421 | 2000 | 78.958µs | 84.042µs | 92.5µs | 198 | 4 | 32882 | 16461 |
| full compress + gain | repetitive | 65543 | 22464 | 1023 | 882.75µs | 909.541µs | 921.083µs | 71 | 9928 | 850685 | 348184 |
| full compress + gain | incompressible | 65561 | 65561 | 1023 | 426µs | 479.833µs | 511.208µs | 147 | 4 | 131162 | 65601 |
| full compress + gain | repetitive | 262168 | 93964 | 255 | 3.4815ms | 3.56575ms | 3.588459ms | 72 | 38536 | 3355811 | 1378648 |
| full compress + gain | incompressible | 262156 | 262156 | 255 | 2.200167ms | 2.287916ms | 2.331875ms | 114 | 4 | 524352 | 262196 |
| full compress + gain | repetitive | 1048585 | 391117 | 100 | 13.85975ms | 14.261792ms | 14.341458ms | 72 | 150376 | 13261469 | 5465231 |
| full compress + gain | incompressible | 1048606 | 1048606 | 100 | 8.601834ms | 8.764583ms | 8.918042ms | 116 | 4 | 2097252 | 1048646 |

### deslag-core bounded execution (64 KiB repetitive, full compress, 200 calls per thread)

Default limits: 4 jobs, 10ms budget.

| limits | concurrency | max jobs | completed/s | completed | busy | budget_exceeded | job_panicked |
| --- | --- | --- | --- | --- | --- | --- | --- |
| default | 1 | 4 | 1090 | 200 | 0.0% | 0.0% | 0.0% |
| max jobs = concurrency | 1 | 1 | 1096 | 200 | 0.0% | 0.0% | 0.0% |
| default | 8 | 4 | 1724 | 800 | 50.0% | 0.0% | 0.0% |
| max jobs = concurrency | 8 | 8 | 1681 | 1600 | 0.0% | 0.0% | 0.0% |
| default | 32 | 4 | 1718 | 800 | 87.5% | 0.0% | 0.0% |
| max jobs = concurrency | 32 | 32 | 1934 | 5359 | 13.5% | 2.8% | 0.0% |

### deslag-core minify_json (concurrency 1, warmup 10)

| input | bytes | changed | samples | p50 | p95 | p99 | MiB/s at p50 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| repetitive | 2131 | yes | 2000 | 3.583µs | 3.834µs | 4.333µs | 567 |
| incompressible | 2071 | no | 2000 | 1.042µs | 1.166µs | 1.25µs | 1895 |
| repetitive | 16468 | yes | 2000 | 26.334µs | 27.541µs | 28.708µs | 596 |
| incompressible | 16421 | no | 2000 | 7.709µs | 7.792µs | 7.834µs | 2031 |
| repetitive | 65627 | yes | 1022 | 103.042µs | 106.167µs | 113.625µs | 607 |
| incompressible | 65561 | no | 1023 | 30.334µs | 36.292µs | 36.708µs | 2061 |
| repetitive | 262193 | yes | 255 | 404.875µs | 410.875µs | 422.25µs | 618 |
| incompressible | 262156 | no | 255 | 120.75µs | 122.125µs | 123.334µs | 2070 |
| repetitive | 1048628 | yes | 100 | 1.617458ms | 1.663291ms | 1.672084ms | 618 |
| incompressible | 1048606 | no | 100 | 480.083µs | 492.084µs | 497.5µs | 2083 |

### deslag-proxy request latency (273190 byte request, concurrency 1, warmup 20, samples 500)

| path | upstream bytes | p50 | p95 | p99 |
| --- | --- | --- | --- | --- |
| direct to mock | 273190 | 89.916µs | 127.25µs | 142.417µs |
| proxy, transparent | 273190 | 106.208µs | 122.75µs | 131.458µs |
| proxy, --minify-json | 244704 | 1.1835ms | 1.246958ms | 1.279625ms |

