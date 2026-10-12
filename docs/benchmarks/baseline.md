# Benchmark baseline

PLAN §13.2 budgets remain provisional. This file records measurements and sets no release thresholds.

## Metadata

| Field | Value |
| --- | --- |
| Date (UTC) | 2026-10-12T00:16:11Z |
| Commit | aae37fa00ad93d89dec9246c0ee40859200b8136 |
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
| no-op (no flag) | 1.4005ms | 1.84075ms | 1.953833ms |
| --minify-json | 1.68675ms | 2.030666ms | 2.147167ms |

### deslag-core stages (concurrency 1, warmup 10)

Corpus `generated-v1`. Tokenizer: method `estimate`, `deslag-estimate-v1`. Samples per row: 64 MiB divided by the input size, clamped to 100..=2000. Allocations, bytes and peak heap come from one call after warmup. A growing realloc counts as one allocation of the added bytes.

| stage | input | bytes | output (bytes; tokens for token estimate) | samples | p50 | p95 | p99 | MiB/s at p50 | allocs/call | alloc bytes/call | peak heap bytes/call |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| minify scan | repetitive | 2074 | 1662 | 2000 | 4.417µs | 5.25µs | 5.583µs | 448 | 42 | 5190 | 2150 |
| minify scan | incompressible | 2071 | 2071 | 2000 | 1.291µs | 1.334µs | 1.625µs | 1530 | 1 | 2071 | 2071 |
| minify scan | repetitive | 16425 | 13283 | 2000 | 27.917µs | 32.75µs | 36.084µs | 561 | 315 | 40289 | 16501 |
| minify scan | incompressible | 16421 | 16421 | 2000 | 7.583µs | 7.625µs | 7.833µs | 2065 | 1 | 16421 | 16421 |
| minify scan | repetitive | 65543 | 53221 | 1023 | 103.625µs | 106.917µs | 118.708µs | 603 | 1233 | 159175 | 65619 |
| minify scan | incompressible | 65561 | 65561 | 1023 | 30.125µs | 30.542µs | 34µs | 2075 | 1 | 65561 | 65561 |
| minify scan | repetitive | 262168 | 214096 | 255 | 406.875µs | 421.166µs | 430.541µs | 614 | 4808 | 627500 | 262244 |
| minify scan | incompressible | 262156 | 262156 | 255 | 116.875µs | 123.5µs | 134.333µs | 2139 | 1 | 262156 | 262156 |
| minify scan | repetitive | 1048585 | 860723 | 100 | 1.599333ms | 1.62525ms | 1.628375ms | 625 | 18787 | 2476321 | 1048661 |
| minify scan | incompressible | 1048606 | 1048606 | 100 | 477.916µs | 493.792µs | 499.667µs | 2092 | 1 | 1048606 | 1048606 |
| table-v1 encode | repetitive | 2074 | 678 | 2000 | 19.708µs | 20.833µs | 22.667µs | 100 | 380 | 28755 | 11460 |
| table-v1 encode | incompressible | 2099 | 1400 | 2000 | 20.416µs | 21.792µs | 23.292µs | 98 | 311 | 25136 | 9869 |
| table-v1 encode | repetitive | 16425 | 5475 | 2000 | 141µs | 149.208µs | 160.916µs | 111 | 2576 | 215861 | 87969 |
| table-v1 encode | incompressible | 16457 | 10831 | 2000 | 146.042µs | 151.334µs | 165.625µs | 107 | 2083 | 188828 | 74915 |
| table-v1 encode | repetitive | 65543 | 22464 | 1023 | 546.625µs | 564.084µs | 573.417µs | 114 | 9928 | 850685 | 348184 |
| table-v1 encode | incompressible | 65575 | 43177 | 1023 | 579.166µs | 599.958µs | 613.584µs | 108 | 8081 | 747660 | 297041 |
| table-v1 encode | repetitive | 262168 | 93964 | 255 | 2.174958ms | 2.239292ms | 2.360417ms | 115 | 38536 | 3355811 | 1378648 |
| table-v1 encode | incompressible | 262169 | 173384 | 255 | 2.325958ms | 2.41375ms | 2.51075ms | 107 | 31799 | 2970932 | 1181267 |
| table-v1 encode | repetitive | 1048585 | 391117 | 100 | 8.549917ms | 8.892ms | 8.979875ms | 117 | 150376 | 13261469 | 5465231 |
| table-v1 encode | incompressible | 1048637 | 695421 | 100 | 11.354ms | 12.010625ms | 12.353ms | 88 | 126247 | 11844524 | 4711223 |
| exact-line-runs | repetitive | 2160 | 218 | 2000 | 1.959µs | 2.042µs | 2.333µs | 1052 | 23 | 3916 | 3816 |
| exact-line-runs | incompressible | 2090 | 2090 | 2000 | 792ns | 833ns | 834ns | 2517 | 7 | 2336 | 2336 |
| exact-line-runs | repetitive | 16416 | 1538 | 2000 | 11.875µs | 12.625µs | 13.583µs | 1318 | 95 | 31288 | 30528 |
| exact-line-runs | incompressible | 16390 | 16390 | 2000 | 4.5µs | 5.666µs | 5.958µs | 3473 | 10 | 18688 | 18688 |
| exact-line-runs | repetitive | 65664 | 6098 | 1022 | 44.458µs | 46.875µs | 50.041µs | 1409 | 327 | 125152 | 122112 |
| exact-line-runs | incompressible | 65560 | 65560 | 1023 | 18.083µs | 18.292µs | 18.542µs | 3458 | 12 | 74752 | 74752 |
| exact-line-runs | repetitive | 262224 | 24298 | 255 | 168.25µs | 180.875µs | 189.083µs | 1486 | 1241 | 500588 | 488448 |
| exact-line-runs | incompressible | 262185 | 262185 | 255 | 64.458µs | 66.583µs | 70.166µs | 3879 | 14 | 299008 | 299008 |
| exact-line-runs | repetitive | 1048896 | 97138 | 100 | 677.5µs | 703.417µs | 715.333µs | 1476 | 4887 | 2002352 | 1953792 |
| exact-line-runs | incompressible | 1048630 | 1048630 | 100 | 255.583µs | 265.708µs | 270.917µs | 3913 | 16 | 1196032 | 1196032 |
| token estimate | repetitive | 2074 | 824 | 2000 | 4.958µs | 5.334µs | 5.625µs | 399 | 0 | 0 | 0 |
| token estimate | incompressible | 2071 | 833 | 2000 | 3.583µs | 3.625µs | 4.292µs | 551 | 0 | 0 | 0 |
| token estimate | repetitive | 16425 | 6284 | 2000 | 40.25µs | 41.5µs | 44.375µs | 389 | 0 | 0 | 0 |
| token estimate | incompressible | 16421 | 6738 | 2000 | 30.375µs | 34.833µs | 39.708µs | 516 | 0 | 0 | 0 |
| token estimate | repetitive | 65543 | 25108 | 1023 | 161.375µs | 168.75µs | 179.75µs | 387 | 0 | 0 | 0 |
| token estimate | incompressible | 65561 | 27124 | 1023 | 175.083µs | 194.542µs | 221.375µs | 357 | 0 | 0 | 0 |
| token estimate | repetitive | 262168 | 103758 | 255 | 606.042µs | 634.25µs | 643.541µs | 413 | 0 | 0 | 0 |
| token estimate | incompressible | 262156 | 108384 | 255 | 882.5µs | 949.083µs | 997.916µs | 283 | 0 | 0 | 0 |
| token estimate | repetitive | 1048585 | 411296 | 100 | 2.365334ms | 2.4965ms | 2.52575ms | 423 | 0 | 0 | 0 |
| token estimate | incompressible | 1048606 | 433420 | 100 | 3.737834ms | 3.922667ms | 3.98325ms | 268 | 0 | 0 | 0 |
| full compress + gain | repetitive | 2074 | 678 | 2000 | 30.834µs | 32.125µs | 36.334µs | 64 | 380 | 28755 | 11460 |
| full compress + gain | incompressible | 2071 | 2071 | 2000 | 8.916µs | 9.5µs | 10.584µs | 222 | 4 | 4182 | 2111 |
| full compress + gain | repetitive | 16425 | 5475 | 2000 | 226.167µs | 234.166µs | 247.542µs | 69 | 2576 | 215861 | 87969 |
| full compress + gain | incompressible | 16421 | 16421 | 2000 | 73µs | 79.042µs | 89.167µs | 215 | 4 | 32882 | 16461 |
| full compress + gain | repetitive | 65543 | 22464 | 1023 | 888.417µs | 914.709µs | 971.542µs | 70 | 9928 | 850685 | 348184 |
| full compress + gain | incompressible | 65561 | 65561 | 1023 | 373.542µs | 445.667µs | 461.292µs | 167 | 4 | 131162 | 65601 |
| full compress + gain | repetitive | 262168 | 93964 | 255 | 3.437875ms | 3.530583ms | 3.558542ms | 73 | 38536 | 3355811 | 1378648 |
| full compress + gain | incompressible | 262156 | 262156 | 255 | 2.018041ms | 2.173709ms | 2.226166ms | 124 | 4 | 524352 | 262196 |
| full compress + gain | repetitive | 1048585 | 391117 | 100 | 13.477333ms | 13.843458ms | 13.998416ms | 74 | 150376 | 13261469 | 5465231 |
| full compress + gain | incompressible | 1048606 | 1048606 | 100 | 8.408708ms | 8.581125ms | 8.661625ms | 119 | 4 | 2097252 | 1048646 |

### deslag-core bounded execution (64 KiB repetitive, full compress, 200 calls per thread)

Default limits: 4 jobs, 10ms budget.

| limits | concurrency | max jobs | completed/s | completed | busy | budget_exceeded | job_panicked |
| --- | --- | --- | --- | --- | --- | --- | --- |
| default | 1 | 4 | 1081 | 200 | 0.0% | 0.0% | 0.0% |
| max jobs = concurrency | 1 | 1 | 1085 | 200 | 0.0% | 0.0% | 0.0% |
| default | 8 | 4 | 1696 | 800 | 50.0% | 0.0% | 0.0% |
| max jobs = concurrency | 8 | 8 | 1651 | 1600 | 0.0% | 0.0% | 0.0% |
| default | 32 | 4 | 1699 | 800 | 87.5% | 0.0% | 0.0% |
| max jobs = concurrency | 32 | 32 | 1935 | 5194 | 16.9% | 1.9% | 0.0% |

### deslag-core minify_json (concurrency 1, warmup 10)

| input | bytes | changed | samples | p50 | p95 | p99 | MiB/s at p50 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| repetitive | 2131 | yes | 2000 | 3.709µs | 4µs | 4.167µs | 548 |
| incompressible | 2071 | no | 2000 | 1.042µs | 1.125µs | 1.167µs | 1895 |
| repetitive | 16468 | yes | 2000 | 27.041µs | 33.917µs | 41.25µs | 581 |
| incompressible | 16421 | no | 2000 | 7.708µs | 7.833µs | 8.334µs | 2032 |
| repetitive | 65627 | yes | 1022 | 104.209µs | 106.917µs | 110.916µs | 601 |
| incompressible | 65561 | no | 1023 | 30.166µs | 30.333µs | 33µs | 2073 |
| repetitive | 262193 | yes | 255 | 416.125µs | 424.75µs | 431.542µs | 601 |
| incompressible | 262156 | no | 255 | 121.834µs | 124.209µs | 126.375µs | 2052 |
| repetitive | 1048628 | yes | 100 | 1.62075ms | 1.653125ms | 1.661708ms | 617 |
| incompressible | 1048606 | no | 100 | 479.542µs | 488.209µs | 491.667µs | 2085 |

### deslag-proxy request latency (273190 byte request, concurrency 1, warmup 20, samples 500)

| path | upstream bytes | p50 | p95 | p99 |
| --- | --- | --- | --- | --- |
| direct to mock | 273190 | 85µs | 117.5µs | 126.917µs |
| proxy, transparent | 273190 | 100.375µs | 119.625µs | 135.833µs |
| proxy, --minify-json | 244704 | 1.195458ms | 1.265ms | 1.34525ms |

