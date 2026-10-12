# Benchmark baseline

PLAN §13.2 budgets remain provisional. This file records measurements and sets no release thresholds.

## Metadata

| Field | Value |
| --- | --- |
| Date (UTC) | 2026-10-12T08:18:20Z |
| Commit | 9afe4e0de38ca58048d0b3bc751c19e6ca06c980 |
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
| no-op (no flag) | 1.44ms | 1.733875ms | 1.858708ms |
| --minify-json | 1.812459ms | 2.07775ms | 2.17375ms |

### deslag-core stages (concurrency 1, warmup 10)

Corpus `generated-v1`. Tokenizer: method `estimate`, `deslag-estimate-v1`. Samples per row: 64 MiB divided by the input size, clamped to 100..=2000. Allocations, bytes and peak heap come from one call after warmup. A growing realloc counts as one allocation of the added bytes.

| stage | input | bytes | output (bytes; tokens for token estimate) | samples | p50 | p95 | p99 | MiB/s at p50 | allocs/call | alloc bytes/call | peak heap bytes/call |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| minify scan | repetitive | 2074 | 1662 | 2000 | 3.542µs | 3.875µs | 4.291µs | 558 | 42 | 5190 | 2150 |
| minify scan | incompressible | 2071 | 2071 | 2000 | 1.042µs | 1.209µs | 1.291µs | 1895 | 1 | 2071 | 2071 |
| minify scan | repetitive | 16425 | 13283 | 2000 | 27µs | 27.584µs | 30.542µs | 580 | 315 | 40289 | 16501 |
| minify scan | incompressible | 16421 | 16421 | 2000 | 7.625µs | 7.75µs | 8.375µs | 2054 | 1 | 16421 | 16421 |
| minify scan | repetitive | 65543 | 53221 | 1023 | 105.917µs | 109.458µs | 115µs | 590 | 1233 | 159175 | 65619 |
| minify scan | incompressible | 65561 | 65561 | 1023 | 30.166µs | 31.292µs | 33.833µs | 2073 | 1 | 65561 | 65561 |
| minify scan | repetitive | 262168 | 214096 | 255 | 409.958µs | 424.833µs | 429.834µs | 610 | 4808 | 627500 | 262244 |
| minify scan | incompressible | 262156 | 262156 | 255 | 120.125µs | 124.667µs | 134.375µs | 2081 | 1 | 262156 | 262156 |
| minify scan | repetitive | 1048585 | 860723 | 100 | 1.603292ms | 1.629209ms | 1.7ms | 624 | 18787 | 2476321 | 1048661 |
| minify scan | incompressible | 1048606 | 1048606 | 100 | 484.083µs | 496.625µs | 508.625µs | 2066 | 1 | 1048606 | 1048606 |
| table-v2 encode, bare array | repetitive | 2074 | 678 | 2000 | 21.209µs | 23.584µs | 27.375µs | 93 | 416 | 40919 | 14436 |
| table-v2 encode, bare array | incompressible | 2099 | 1400 | 2000 | 23.75µs | 25.583µs | 27.834µs | 84 | 347 | 35391 | 12203 |
| table-v2 encode, bare array | repetitive | 16425 | 5475 | 2000 | 146.167µs | 153.584µs | 162.667µs | 107 | 2627 | 312248 | 112600 |
| table-v2 encode, bare array | incompressible | 16457 | 10831 | 2000 | 161µs | 167.792µs | 178.292µs | 97 | 2137 | 270259 | 96152 |
| table-v2 encode, bare array | repetitive | 65543 | 22464 | 1023 | 562.541µs | 584.917µs | 603.708µs | 111 | 9989 | 1235860 | 447171 |
| table-v2 encode, bare array | incompressible | 65575 | 43177 | 1023 | 624.916µs | 648.125µs | 660µs | 100 | 8147 | 1072821 | 383312 |
| table-v2 encode, bare array | repetitive | 262168 | 93964 | 255 | 2.217625ms | 2.283541ms | 2.332083ms | 113 | 38607 | 4897205 | 1775960 |
| table-v2 encode, bare array | incompressible | 262169 | 173384 | 255 | 2.482084ms | 2.602ms | 2.717125ms | 101 | 31877 | 4270741 | 1529697 |
| table-v2 encode, bare array | repetitive | 1048585 | 391117 | 100 | 8.840583ms | 9.116667ms | 9.402917ms | 113 | 150457 | 19430820 | 7058530 |
| table-v2 encode, bare array | incompressible | 1048637 | 695421 | 100 | 9.84825ms | 10.201791ms | 10.3825ms | 102 | 126337 | 17042435 | 6111418 |
| exact-line-runs | repetitive | 2160 | 218 | 2000 | 1.916µs | 1.959µs | 2.125µs | 1075 | 23 | 3916 | 3816 |
| exact-line-runs | incompressible | 2090 | 2090 | 2000 | 792ns | 875ns | 958ns | 2517 | 7 | 2336 | 2336 |
| exact-line-runs | repetitive | 16416 | 1538 | 2000 | 11.375µs | 11.792µs | 13.459µs | 1376 | 95 | 31288 | 30528 |
| exact-line-runs | incompressible | 16390 | 16390 | 2000 | 4.583µs | 4.666µs | 4.958µs | 3411 | 10 | 18688 | 18688 |
| exact-line-runs | repetitive | 65664 | 6098 | 1022 | 44.041µs | 46.042µs | 50.375µs | 1422 | 327 | 125152 | 122112 |
| exact-line-runs | incompressible | 65560 | 65560 | 1023 | 17.458µs | 18.25µs | 24.417µs | 3581 | 12 | 74752 | 74752 |
| exact-line-runs | repetitive | 262224 | 24298 | 255 | 169.416µs | 176.584µs | 187.708µs | 1476 | 1241 | 500588 | 488448 |
| exact-line-runs | incompressible | 262185 | 262185 | 255 | 63.542µs | 65.583µs | 70.75µs | 3935 | 14 | 299008 | 299008 |
| exact-line-runs | repetitive | 1048896 | 97138 | 100 | 673.542µs | 710.5µs | 720.791µs | 1485 | 4887 | 2002352 | 1953792 |
| exact-line-runs | incompressible | 1048630 | 1048630 | 100 | 249.5µs | 273.584µs | 282.083µs | 4008 | 16 | 1196032 | 1196032 |
| line-templates | repetitive | 2086 | 1341 | 2000 | 15.917µs | 16.584µs | 19.375µs | 125 | 420 | 31395 | 15279 |
| line-templates | incompressible | 2090 | 2090 | 2000 | 6.958µs | 7.333µs | 8.583µs | 286 | 256 | 15895 | 10253 |
| line-templates | repetitive | 16433 | 10169 | 2000 | 116.333µs | 121.167µs | 136.166µs | 135 | 3072 | 245095 | 117995 |
| line-templates | incompressible | 16390 | 16352 | 2000 | 57.333µs | 59.375µs | 63.625µs | 273 | 1958 | 152137 | 78512 |
| line-templates | repetitive | 65567 | 40398 | 1023 | 459.625µs | 479.417µs | 489.083µs | 136 | 12122 | 961701 | 454416 |
| line-templates | incompressible | 65560 | 65300 | 1023 | 236.75µs | 248.625µs | 263.958µs | 264 | 8205 | 629474 | 310272 |
| line-templates | repetitive | 262178 | 161362 | 255 | 1.821416ms | 1.869041ms | 1.894875ms | 137 | 48309 | 3844243 | 1815788 |
| line-templates | incompressible | 262185 | 260833 | 255 | 955.709µs | 993.458µs | 1.01125ms | 262 | 32949 | 2538608 | 1238459 |
| line-templates | repetitive | 1048630 | 645227 | 100 | 8.889625ms | 15.707042ms | 16.44075ms | 112 | 193033 | 15374461 | 7261285 |
| line-templates | incompressible | 1048630 | 1043353 | 100 | 4.20325ms | 11.679833ms | 29.973042ms | 238 | 130890 | 10145537 | 4969458 |
| token estimate | repetitive | 2074 | 824 | 2000 | 3.417µs | 3.625µs | 3.709µs | 579 | 0 | 0 | 0 |
| token estimate | incompressible | 2071 | 833 | 2000 | 2.792µs | 2.917µs | 3.25µs | 707 | 0 | 0 | 0 |
| token estimate | repetitive | 16425 | 6284 | 2000 | 28.417µs | 28.875µs | 32.459µs | 551 | 0 | 0 | 0 |
| token estimate | incompressible | 16421 | 6738 | 2000 | 23.5µs | 24.917µs | 29.458µs | 666 | 0 | 0 | 0 |
| token estimate | repetitive | 65543 | 25108 | 1023 | 111.125µs | 116.833µs | 126.875µs | 562 | 0 | 0 | 0 |
| token estimate | incompressible | 65561 | 27124 | 1023 | 134.459µs | 198.5µs | 218.375µs | 465 | 0 | 0 | 0 |
| token estimate | repetitive | 262168 | 103758 | 255 | 446.5µs | 474.833µs | 502.875µs | 560 | 0 | 0 | 0 |
| token estimate | incompressible | 262156 | 108384 | 255 | 847.333µs | 1.310208ms | 1.370708ms | 295 | 0 | 0 | 0 |
| token estimate | repetitive | 1048585 | 411296 | 100 | 1.788ms | 1.961458ms | 2.027958ms | 559 | 0 | 0 | 0 |
| token estimate | incompressible | 1048606 | 433420 | 100 | 3.399458ms | 3.898542ms | 4.478666ms | 294 | 0 | 0 | 0 |
| full compress + gain | repetitive | 2074 | 678 | 2000 | 36.667µs | 41.708µs | 65.75µs | 54 | 416 | 40919 | 14436 |
| full compress + gain | incompressible | 2071 | 2071 | 2000 | 8.709µs | 8.834µs | 9.334µs | 227 | 5 | 4190 | 2111 |
| full compress + gain | repetitive | 16425 | 5475 | 2000 | 264.667µs | 275.167µs | 354.166µs | 59 | 2627 | 312248 | 112600 |
| full compress + gain | incompressible | 16421 | 16421 | 2000 | 64.708µs | 97.083µs | 125.292µs | 242 | 5 | 32890 | 16461 |
| full compress + gain | repetitive | 65543 | 22464 | 1023 | 822.959µs | 855.542µs | 887.958µs | 76 | 9989 | 1235860 | 447171 |
| full compress + gain | incompressible | 65561 | 65561 | 1023 | 331.667µs | 375.792µs | 433.75µs | 189 | 5 | 131170 | 65601 |
| full compress + gain | repetitive | 262168 | 93964 | 255 | 3.307125ms | 3.507167ms | 3.841833ms | 76 | 38607 | 4897205 | 1775960 |
| full compress + gain | incompressible | 262156 | 262156 | 255 | 1.67975ms | 1.746083ms | 1.773167ms | 149 | 5 | 524360 | 262196 |
| full compress + gain | repetitive | 1048585 | 391117 | 100 | 12.925917ms | 13.195041ms | 13.264416ms | 77 | 150457 | 19430820 | 7058530 |
| full compress + gain | incompressible | 1048606 | 1048606 | 100 | 7.176291ms | 8.637417ms | 9.340125ms | 139 | 5 | 2097260 | 1048646 |

### deslag-core bounded execution (64 KiB repetitive, full compress, 200 calls per thread)

Default limits: 4 jobs, 10ms budget.

| limits | concurrency | max jobs | completed/s | completed | busy | budget_exceeded | job_panicked |
| --- | --- | --- | --- | --- | --- | --- | --- |
| default | 1 | 4 | 1053 | 200 | 0.0% | 0.0% | 0.0% |
| max jobs = concurrency | 1 | 1 | 1088 | 200 | 0.0% | 0.0% | 0.0% |
| default | 8 | 4 | 1655 | 800 | 50.0% | 0.0% | 0.0% |
| max jobs = concurrency | 8 | 8 | 1713 | 1600 | 0.0% | 0.0% | 0.0% |
| default | 32 | 4 | 1765 | 800 | 87.5% | 0.0% | 0.0% |
| max jobs = concurrency | 32 | 32 | 2047 | 4180 | 34.2% | 0.5% | 0.0% |

### deslag-core minify_json (concurrency 1, warmup 10)

| input | bytes | changed | samples | p50 | p95 | p99 | MiB/s at p50 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| repetitive | 2131 | yes | 2000 | 3.625µs | 3.834µs | 4.125µs | 561 |
| incompressible | 2071 | no | 2000 | 1.042µs | 1.166µs | 1.291µs | 1895 |
| repetitive | 16468 | yes | 2000 | 26.625µs | 27.208µs | 27.791µs | 590 |
| incompressible | 16421 | no | 2000 | 7.75µs | 8.084µs | 8.5µs | 2021 |
| repetitive | 65627 | yes | 1022 | 105.125µs | 135.833µs | 178.709µs | 595 |
| incompressible | 65561 | no | 1023 | 30.917µs | 41.166µs | 52.917µs | 2022 |
| repetitive | 262193 | yes | 255 | 411.709µs | 428.917µs | 450.625µs | 607 |
| incompressible | 262156 | no | 255 | 121.667µs | 127.917µs | 143.875µs | 2055 |
| repetitive | 1048628 | yes | 100 | 1.630792ms | 1.664959ms | 1.714875ms | 613 |
| incompressible | 1048606 | no | 100 | 484.875µs | 504.375µs | 521µs | 2062 |

### deslag-proxy request latency (273190 byte request, concurrency 1, warmup 20, samples 500)

| path | upstream bytes | p50 | p95 | p99 |
| --- | --- | --- | --- | --- |
| direct to mock | 273190 | 83µs | 101.625µs | 108.666µs |
| proxy, transparent | 273190 | 98.916µs | 115µs | 127.042µs |
| proxy, --minify-json | 244704 | 1.271958ms | 1.330292ms | 1.562041ms |

