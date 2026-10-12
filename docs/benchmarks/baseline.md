# Benchmark baseline

PLAN §13.2 budgets remain provisional. This file records measurements and sets no release thresholds.

## Metadata

| Field | Value |
| --- | --- |
| Date (UTC) | 2026-10-12T00:28:51Z |
| Commit | e04ccbd27d75ee6dd9f093b1eeaac10477eb158a |
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
| no-op (no flag) | 1.59275ms | 1.753667ms | 1.856959ms |
| --minify-json | 1.925125ms | 2.1675ms | 2.666375ms |

### deslag-core stages (concurrency 1, warmup 10)

Corpus `generated-v1`. Tokenizer: method `estimate`, `deslag-estimate-v1`. Samples per row: 64 MiB divided by the input size, clamped to 100..=2000. Allocations, bytes and peak heap come from one call after warmup. A growing realloc counts as one allocation of the added bytes.

| stage | input | bytes | output (bytes; tokens for token estimate) | samples | p50 | p95 | p99 | MiB/s at p50 | allocs/call | alloc bytes/call | peak heap bytes/call |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| minify scan | repetitive | 2074 | 1662 | 2000 | 3.625µs | 4µs | 4.292µs | 546 | 42 | 5190 | 2150 |
| minify scan | incompressible | 2071 | 2071 | 2000 | 1.208µs | 1.291µs | 1.459µs | 1635 | 1 | 2071 | 2071 |
| minify scan | repetitive | 16425 | 13283 | 2000 | 26.958µs | 28µs | 29.375µs | 581 | 315 | 40289 | 16501 |
| minify scan | incompressible | 16421 | 16421 | 2000 | 7.667µs | 9.042µs | 9.125µs | 2043 | 1 | 16421 | 16421 |
| minify scan | repetitive | 65543 | 53221 | 1023 | 105.792µs | 113.083µs | 115.5µs | 591 | 1233 | 159175 | 65619 |
| minify scan | incompressible | 65561 | 65561 | 1023 | 30.708µs | 31.541µs | 32.625µs | 2036 | 1 | 65561 | 65561 |
| minify scan | repetitive | 262168 | 214096 | 255 | 417.25µs | 434.875µs | 439.417µs | 599 | 4808 | 627500 | 262244 |
| minify scan | incompressible | 262156 | 262156 | 255 | 121.834µs | 124.125µs | 126.791µs | 2052 | 1 | 262156 | 262156 |
| minify scan | repetitive | 1048585 | 860723 | 100 | 1.684458ms | 1.806542ms | 1.814166ms | 594 | 18787 | 2476321 | 1048661 |
| minify scan | incompressible | 1048606 | 1048606 | 100 | 547.125µs | 621.625µs | 646.916µs | 1828 | 1 | 1048606 | 1048606 |
| table-v1 encode | repetitive | 2074 | 678 | 2000 | 20.334µs | 27µs | 30.083µs | 97 | 380 | 28755 | 11460 |
| table-v1 encode | incompressible | 2099 | 1400 | 2000 | 21.208µs | 22.375µs | 25.542µs | 94 | 311 | 25136 | 9869 |
| table-v1 encode | repetitive | 16425 | 5475 | 2000 | 143.583µs | 151.125µs | 167.5µs | 109 | 2576 | 215861 | 87969 |
| table-v1 encode | incompressible | 16457 | 10831 | 2000 | 153.791µs | 177.541µs | 180.458µs | 102 | 2083 | 188828 | 74915 |
| table-v1 encode | repetitive | 65543 | 22464 | 1023 | 569.208µs | 596.833µs | 632.875µs | 110 | 9928 | 850685 | 348184 |
| table-v1 encode | incompressible | 65575 | 43177 | 1023 | 596.292µs | 666.75µs | 672.667µs | 105 | 8081 | 747660 | 297041 |
| table-v1 encode | repetitive | 262168 | 93964 | 255 | 2.23425ms | 2.51275ms | 2.787541ms | 112 | 38536 | 3355811 | 1378648 |
| table-v1 encode | incompressible | 262169 | 173384 | 255 | 2.413625ms | 2.497791ms | 2.577084ms | 104 | 31799 | 2970932 | 1181267 |
| table-v1 encode | repetitive | 1048585 | 391117 | 100 | 8.852541ms | 9.036459ms | 9.315208ms | 113 | 150376 | 13261469 | 5465231 |
| table-v1 encode | incompressible | 1048637 | 695421 | 100 | 9.562208ms | 10.884875ms | 11.945792ms | 105 | 126247 | 11844524 | 4711223 |
| exact-line-runs | repetitive | 2160 | 218 | 2000 | 2.416µs | 2.625µs | 3.084µs | 853 | 23 | 3916 | 3816 |
| exact-line-runs | incompressible | 2090 | 2090 | 2000 | 959ns | 1.125µs | 1.292µs | 2078 | 7 | 2336 | 2336 |
| exact-line-runs | repetitive | 16416 | 1538 | 2000 | 11.875µs | 15.208µs | 15.916µs | 1318 | 95 | 31288 | 30528 |
| exact-line-runs | incompressible | 16390 | 16390 | 2000 | 5.708µs | 6.292µs | 6.875µs | 2738 | 10 | 18688 | 18688 |
| exact-line-runs | repetitive | 65664 | 6098 | 1022 | 52.5µs | 59.416µs | 60.625µs | 1193 | 327 | 125152 | 122112 |
| exact-line-runs | incompressible | 65560 | 65560 | 1023 | 21.459µs | 22.917µs | 24.542µs | 2914 | 12 | 74752 | 74752 |
| exact-line-runs | repetitive | 262224 | 24298 | 255 | 198.541µs | 228.292µs | 230.292µs | 1260 | 1241 | 500588 | 488448 |
| exact-line-runs | incompressible | 262185 | 262185 | 255 | 68µs | 71.209µs | 74.375µs | 3677 | 14 | 299008 | 299008 |
| exact-line-runs | repetitive | 1048896 | 97138 | 100 | 750.625µs | 825.417µs | 843.25µs | 1333 | 4887 | 2002352 | 1953792 |
| exact-line-runs | incompressible | 1048630 | 1048630 | 100 | 256.625µs | 280.708µs | 285.041µs | 3897 | 16 | 1196032 | 1196032 |
| line-templates | repetitive | 2086 | 1341 | 2000 | 13.167µs | 14.125µs | 14.75µs | 151 | 348 | 30132 | 14696 |
| line-templates | incompressible | 2090 | 2090 | 2000 | 8.666µs | 9.416µs | 10.083µs | 230 | 279 | 18832 | 3944 |
| line-templates | repetitive | 16433 | 10169 | 2000 | 94.375µs | 102.5µs | 103.75µs | 166 | 2520 | 236856 | 115236 |
| line-templates | incompressible | 16390 | 16352 | 2000 | 73.875µs | 82.875µs | 86µs | 212 | 2331 | 181592 | 46848 |
| line-templates | repetitive | 65567 | 40398 | 1023 | 375.292µs | 435.125µs | 481.375µs | 167 | 9926 | 929582 | 444217 |
| line-templates | incompressible | 65560 | 65300 | 1023 | 300.25µs | 333.333µs | 414.375µs | 208 | 9562 | 740490 | 187660 |
| line-templates | repetitive | 262178 | 161362 | 255 | 1.483375ms | 1.605584ms | 1.703583ms | 169 | 39535 | 3716560 | 1775805 |
| line-templates | incompressible | 262185 | 260833 | 255 | 1.213042ms | 1.320875ms | 1.68675ms | 206 | 37721 | 2954081 | 749836 |
| line-templates | repetitive | 1048630 | 645227 | 100 | 5.873084ms | 6.949625ms | 6.999125ms | 170 | 157947 | 14864522 | 7102166 |
| line-templates | incompressible | 1048630 | 1043353 | 100 | 4.826666ms | 4.985541ms | 5.326125ms | 207 | 150016 | 11812511 | 2998540 |
| token estimate | repetitive | 2074 | 824 | 2000 | 3.542µs | 3.584µs | 3.666µs | 558 | 0 | 0 | 0 |
| token estimate | incompressible | 2071 | 833 | 2000 | 2.75µs | 2.792µs | 3.083µs | 718 | 0 | 0 | 0 |
| token estimate | repetitive | 16425 | 6284 | 2000 | 27.125µs | 27.458µs | 30.125µs | 577 | 0 | 0 | 0 |
| token estimate | incompressible | 16421 | 6738 | 2000 | 22.375µs | 22.625µs | 26.208µs | 700 | 0 | 0 | 0 |
| token estimate | repetitive | 65543 | 25108 | 1023 | 111.708µs | 116.5µs | 123.5µs | 560 | 0 | 0 | 0 |
| token estimate | incompressible | 65561 | 27124 | 1023 | 107.625µs | 121.875µs | 177.792µs | 581 | 0 | 0 | 0 |
| token estimate | repetitive | 262168 | 103758 | 255 | 431.125µs | 461.417µs | 469.416µs | 580 | 0 | 0 | 0 |
| token estimate | incompressible | 262156 | 108384 | 255 | 640.583µs | 689.5µs | 731.875µs | 390 | 0 | 0 | 0 |
| token estimate | repetitive | 1048585 | 411296 | 100 | 1.735875ms | 1.805334ms | 1.818125ms | 576 | 0 | 0 | 0 |
| token estimate | incompressible | 1048606 | 433420 | 100 | 2.854458ms | 2.953583ms | 2.970042ms | 350 | 0 | 0 | 0 |
| full compress + gain | repetitive | 2074 | 678 | 2000 | 27.833µs | 29.292µs | 32.875µs | 71 | 380 | 28755 | 11460 |
| full compress + gain | incompressible | 2071 | 2071 | 2000 | 7.583µs | 7.792µs | 9.208µs | 260 | 4 | 4182 | 2111 |
| full compress + gain | repetitive | 16425 | 5475 | 2000 | 204.083µs | 213.416µs | 224.875µs | 77 | 2576 | 215861 | 87969 |
| full compress + gain | incompressible | 16421 | 16421 | 2000 | 59.959µs | 62.125µs | 66.417µs | 261 | 4 | 32882 | 16461 |
| full compress + gain | repetitive | 65543 | 22464 | 1023 | 813.959µs | 844.042µs | 858.917µs | 77 | 9928 | 850685 | 348184 |
| full compress + gain | incompressible | 65561 | 65561 | 1023 | 277.208µs | 331.291µs | 361.875µs | 226 | 4 | 131162 | 65601 |
| full compress + gain | repetitive | 262168 | 93964 | 255 | 3.212125ms | 3.308917ms | 3.39875ms | 78 | 38536 | 3355811 | 1378648 |
| full compress + gain | incompressible | 262156 | 262156 | 255 | 1.7605ms | 2.120541ms | 2.478375ms | 142 | 4 | 524352 | 262196 |
| full compress + gain | repetitive | 1048585 | 391117 | 100 | 13.623ms | 15.374542ms | 15.750167ms | 73 | 150376 | 13261469 | 5465231 |
| full compress + gain | incompressible | 1048606 | 1048606 | 100 | 7.514375ms | 8.056167ms | 8.241625ms | 133 | 4 | 2097252 | 1048646 |

### deslag-core bounded execution (64 KiB repetitive, full compress, 200 calls per thread)

Default limits: 4 jobs, 10ms budget.

| limits | concurrency | max jobs | completed/s | completed | busy | budget_exceeded | job_panicked |
| --- | --- | --- | --- | --- | --- | --- | --- |
| default | 1 | 4 | 1040 | 200 | 0.0% | 0.0% | 0.0% |
| max jobs = concurrency | 1 | 1 | 1057 | 200 | 0.0% | 0.0% | 0.0% |
| default | 8 | 4 | 1520 | 800 | 50.0% | 0.0% | 0.0% |
| max jobs = concurrency | 8 | 8 | 1692 | 1600 | 0.0% | 0.0% | 0.0% |
| default | 32 | 4 | 1649 | 800 | 87.5% | 0.0% | 0.0% |
| max jobs = concurrency | 32 | 32 | 1978 | 4185 | 34.1% | 0.5% | 0.0% |

### deslag-core minify_json (concurrency 1, warmup 10)

| input | bytes | changed | samples | p50 | p95 | p99 | MiB/s at p50 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| repetitive | 2131 | yes | 2000 | 3.542µs | 3.875µs | 4.25µs | 574 |
| incompressible | 2071 | no | 2000 | 1.042µs | 1.167µs | 1.292µs | 1895 |
| repetitive | 16468 | yes | 2000 | 26.333µs | 27.25µs | 28.084µs | 596 |
| incompressible | 16421 | no | 2000 | 9.208µs | 9.334µs | 9.958µs | 1701 |
| repetitive | 65627 | yes | 1022 | 103.208µs | 105.792µs | 109.25µs | 606 |
| incompressible | 65561 | no | 1023 | 30.5µs | 31.333µs | 32.458µs | 2050 |
| repetitive | 262193 | yes | 255 | 405.833µs | 413.833µs | 425.333µs | 616 |
| incompressible | 262156 | no | 255 | 144.792µs | 150.542µs | 152.209µs | 1727 |
| repetitive | 1048628 | yes | 100 | 1.624625ms | 1.644041ms | 1.660417ms | 616 |
| incompressible | 1048606 | no | 100 | 491.709µs | 496.334µs | 500.125µs | 2034 |

### deslag-proxy request latency (273190 byte request, concurrency 1, warmup 20, samples 500)

| path | upstream bytes | p50 | p95 | p99 |
| --- | --- | --- | --- | --- |
| direct to mock | 273190 | 83.25µs | 104.625µs | 121.458µs |
| proxy, transparent | 273190 | 102.5µs | 125.834µs | 140.208µs |
| proxy, --minify-json | 244704 | 1.26325ms | 1.319875ms | 1.3635ms |

