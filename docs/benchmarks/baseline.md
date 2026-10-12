# Benchmark baseline

PLAN §13.2 budgets remain provisional. This file records measurements and sets no release thresholds.

## Metadata

| Field | Value |
| --- | --- |
| Date (UTC) | 2026-10-12T07:52:39Z |
| Commit | ffe9c45673bfcf56aeaa6c0b41e40b9f2779c293 |
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
| no-op (no flag) | 1.704791ms | 2.104083ms | 2.584459ms |
| --minify-json | 2.08975ms | 2.300125ms | 2.508125ms |

### deslag-core stages (concurrency 1, warmup 10)

Corpus `generated-v1`. Tokenizer: method `estimate`, `deslag-estimate-v1`. Samples per row: 64 MiB divided by the input size, clamped to 100..=2000. Allocations, bytes and peak heap come from one call after warmup. A growing realloc counts as one allocation of the added bytes.

| stage | input | bytes | output (bytes; tokens for token estimate) | samples | p50 | p95 | p99 | MiB/s at p50 | allocs/call | alloc bytes/call | peak heap bytes/call |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| minify scan | repetitive | 2074 | 1662 | 2000 | 4.333µs | 7.875µs | 8.125µs | 456 | 42 | 5190 | 2150 |
| minify scan | incompressible | 2071 | 2071 | 2000 | 1.75µs | 2.083µs | 2.167µs | 1129 | 1 | 2071 | 2071 |
| minify scan | repetitive | 16425 | 13283 | 2000 | 30µs | 40.25µs | 44.75µs | 522 | 315 | 40289 | 16501 |
| minify scan | incompressible | 16421 | 16421 | 2000 | 8.042µs | 9.25µs | 9.958µs | 1947 | 1 | 16421 | 16421 |
| minify scan | repetitive | 65543 | 53221 | 1023 | 104.875µs | 110.625µs | 118.083µs | 596 | 1233 | 159175 | 65619 |
| minify scan | incompressible | 65561 | 65561 | 1023 | 30.5µs | 37.375µs | 39.25µs | 2050 | 1 | 65561 | 65561 |
| minify scan | repetitive | 262168 | 214096 | 255 | 415.583µs | 439.458µs | 443.75µs | 602 | 4808 | 627500 | 262244 |
| minify scan | incompressible | 262156 | 262156 | 255 | 120.792µs | 127.25µs | 129.291µs | 2070 | 1 | 262156 | 262156 |
| minify scan | repetitive | 1048585 | 860723 | 100 | 1.704291ms | 1.772459ms | 1.866958ms | 587 | 18787 | 2476321 | 1048661 |
| minify scan | incompressible | 1048606 | 1048606 | 100 | 484.333µs | 493.667µs | 500.208µs | 2065 | 1 | 1048606 | 1048606 |
| table-v2 encode, bare array | repetitive | 2074 | 678 | 2000 | 21.458µs | 23µs | 24.584µs | 92 | 416 | 40919 | 14436 |
| table-v2 encode, bare array | incompressible | 2099 | 1400 | 2000 | 23.625µs | 25.708µs | 27.916µs | 85 | 347 | 35391 | 12203 |
| table-v2 encode, bare array | repetitive | 16425 | 5475 | 2000 | 149.916µs | 164.042µs | 201.417µs | 104 | 2627 | 312248 | 112600 |
| table-v2 encode, bare array | incompressible | 16457 | 10831 | 2000 | 166.958µs | 174.625µs | 182.916µs | 94 | 2137 | 270259 | 96152 |
| table-v2 encode, bare array | repetitive | 65543 | 22464 | 1023 | 581.375µs | 684.75µs | 793.5µs | 108 | 9989 | 1235860 | 447171 |
| table-v2 encode, bare array | incompressible | 65575 | 43177 | 1023 | 649.708µs | 715.417µs | 885.958µs | 96 | 8147 | 1072821 | 383312 |
| table-v2 encode, bare array | repetitive | 262168 | 93964 | 255 | 2.271417ms | 2.353709ms | 2.517916ms | 110 | 38607 | 4897205 | 1775960 |
| table-v2 encode, bare array | incompressible | 262169 | 173384 | 255 | 2.622041ms | 2.85725ms | 3.24375ms | 95 | 31877 | 4270741 | 1529697 |
| table-v2 encode, bare array | repetitive | 1048585 | 391117 | 100 | 8.918ms | 9.127375ms | 9.279333ms | 112 | 150457 | 19430820 | 7058530 |
| table-v2 encode, bare array | incompressible | 1048637 | 695421 | 100 | 10.129417ms | 10.461833ms | 10.714208ms | 99 | 126337 | 17042435 | 6111418 |
| exact-line-runs | repetitive | 2160 | 218 | 2000 | 1.958µs | 2.083µs | 2.5µs | 1052 | 23 | 3916 | 3816 |
| exact-line-runs | incompressible | 2090 | 2090 | 2000 | 792ns | 875ns | 1µs | 2517 | 7 | 2336 | 2336 |
| exact-line-runs | repetitive | 16416 | 1538 | 2000 | 11.875µs | 12.458µs | 17.125µs | 1318 | 95 | 31288 | 30528 |
| exact-line-runs | incompressible | 16390 | 16390 | 2000 | 4.834µs | 5.042µs | 6.167µs | 3233 | 10 | 18688 | 18688 |
| exact-line-runs | repetitive | 65664 | 6098 | 1022 | 45.542µs | 48.084µs | 55.792µs | 1375 | 327 | 125152 | 122112 |
| exact-line-runs | incompressible | 65560 | 65560 | 1023 | 17.375µs | 18.375µs | 20.625µs | 3598 | 12 | 74752 | 74752 |
| exact-line-runs | repetitive | 262224 | 24298 | 255 | 172.292µs | 182.625µs | 191.125µs | 1451 | 1241 | 500588 | 488448 |
| exact-line-runs | incompressible | 262185 | 262185 | 255 | 65.958µs | 70.584µs | 77.125µs | 3791 | 14 | 299008 | 299008 |
| exact-line-runs | repetitive | 1048896 | 97138 | 100 | 686.667µs | 712.833µs | 725.292µs | 1457 | 4887 | 2002352 | 1953792 |
| exact-line-runs | incompressible | 1048630 | 1048630 | 100 | 272.125µs | 294.667µs | 314.166µs | 3675 | 16 | 1196032 | 1196032 |
| line-templates | repetitive | 2086 | 1341 | 2000 | 16.042µs | 17.75µs | 23.125µs | 124 | 420 | 31395 | 15279 |
| line-templates | incompressible | 2090 | 2090 | 2000 | 7.041µs | 7.833µs | 10.375µs | 283 | 256 | 15895 | 10253 |
| line-templates | repetitive | 16433 | 10169 | 2000 | 117.625µs | 128.416µs | 136.584µs | 133 | 3072 | 245095 | 117995 |
| line-templates | incompressible | 16390 | 16352 | 2000 | 56.708µs | 63.125µs | 71.125µs | 276 | 1958 | 152137 | 78512 |
| line-templates | repetitive | 65567 | 40398 | 1023 | 461.458µs | 481.959µs | 495.25µs | 136 | 12122 | 961701 | 454416 |
| line-templates | incompressible | 65560 | 65300 | 1023 | 234.459µs | 253.292µs | 263.958µs | 267 | 8205 | 629474 | 310272 |
| line-templates | repetitive | 262178 | 161362 | 255 | 1.82025ms | 1.865875ms | 1.921291ms | 137 | 48309 | 3844243 | 1815788 |
| line-templates | incompressible | 262185 | 260833 | 255 | 952µs | 1.006917ms | 1.032708ms | 263 | 32949 | 2538608 | 1238459 |
| line-templates | repetitive | 1048630 | 645227 | 100 | 7.426834ms | 7.755ms | 7.880417ms | 135 | 193033 | 15374461 | 7261285 |
| line-templates | incompressible | 1048630 | 1043353 | 100 | 3.818917ms | 3.938792ms | 4.002958ms | 262 | 130890 | 10145537 | 4969458 |
| token estimate | repetitive | 2074 | 824 | 2000 | 3.5µs | 3.625µs | 4.25µs | 565 | 0 | 0 | 0 |
| token estimate | incompressible | 2071 | 833 | 2000 | 2.75µs | 2.875µs | 3.209µs | 718 | 0 | 0 | 0 |
| token estimate | repetitive | 16425 | 6284 | 2000 | 27.375µs | 28.291µs | 30.166µs | 572 | 0 | 0 | 0 |
| token estimate | incompressible | 16421 | 6738 | 2000 | 22.584µs | 23.708µs | 28.916µs | 693 | 0 | 0 | 0 |
| token estimate | repetitive | 65543 | 25108 | 1023 | 109.291µs | 118.208µs | 121.958µs | 572 | 0 | 0 | 0 |
| token estimate | incompressible | 65561 | 27124 | 1023 | 109.958µs | 150.958µs | 210.25µs | 569 | 0 | 0 | 0 |
| token estimate | repetitive | 262168 | 103758 | 255 | 432.167µs | 456.875µs | 468.459µs | 579 | 0 | 0 | 0 |
| token estimate | incompressible | 262156 | 108384 | 255 | 693.75µs | 755.5µs | 775.667µs | 360 | 0 | 0 | 0 |
| token estimate | repetitive | 1048585 | 411296 | 100 | 1.705041ms | 1.78275ms | 1.823667ms | 587 | 0 | 0 | 0 |
| token estimate | incompressible | 1048606 | 433420 | 100 | 3.010209ms | 3.097041ms | 3.152833ms | 332 | 0 | 0 | 0 |
| full compress + gain | repetitive | 2074 | 678 | 2000 | 29.375µs | 32.666µs | 39.792µs | 67 | 416 | 40919 | 14436 |
| full compress + gain | incompressible | 2071 | 2071 | 2000 | 7.916µs | 8.292µs | 10.167µs | 250 | 5 | 4190 | 2111 |
| full compress + gain | repetitive | 16425 | 5475 | 2000 | 213.292µs | 223.834µs | 236.041µs | 73 | 2627 | 312248 | 112600 |
| full compress + gain | incompressible | 16421 | 16421 | 2000 | 62.667µs | 65.292µs | 69.917µs | 250 | 5 | 32890 | 16461 |
| full compress + gain | repetitive | 65543 | 22464 | 1023 | 829.458µs | 859.958µs | 878.292µs | 75 | 9989 | 1235860 | 447171 |
| full compress + gain | incompressible | 65561 | 65561 | 1023 | 275.916µs | 315.083µs | 415.333µs | 227 | 5 | 131170 | 65601 |
| full compress + gain | repetitive | 262168 | 93964 | 255 | 3.317833ms | 3.560416ms | 4.185541ms | 75 | 38607 | 4897205 | 1775960 |
| full compress + gain | incompressible | 262156 | 262156 | 255 | 1.559792ms | 1.719709ms | 1.774417ms | 160 | 5 | 524360 | 262196 |
| full compress + gain | repetitive | 1048585 | 391117 | 100 | 13.053375ms | 13.272ms | 13.338333ms | 77 | 150457 | 19430820 | 7058530 |
| full compress + gain | incompressible | 1048606 | 1048606 | 100 | 6.912792ms | 7.234625ms | 7.30325ms | 145 | 5 | 2097260 | 1048646 |

### deslag-core bounded execution (64 KiB repetitive, full compress, 200 calls per thread)

Default limits: 4 jobs, 10ms budget.

| limits | concurrency | max jobs | completed/s | completed | busy | budget_exceeded | job_panicked |
| --- | --- | --- | --- | --- | --- | --- | --- |
| default | 1 | 4 | 1132 | 200 | 0.0% | 0.0% | 0.0% |
| max jobs = concurrency | 1 | 1 | 1156 | 200 | 0.0% | 0.0% | 0.0% |
| default | 8 | 4 | 1737 | 800 | 50.0% | 0.0% | 0.0% |
| max jobs = concurrency | 8 | 8 | 1736 | 1600 | 0.0% | 0.0% | 0.0% |
| default | 32 | 4 | 1777 | 800 | 87.5% | 0.0% | 0.0% |
| max jobs = concurrency | 32 | 32 | 2019 | 4159 | 34.1% | 0.9% | 0.0% |

### deslag-core minify_json (concurrency 1, warmup 10)

| input | bytes | changed | samples | p50 | p95 | p99 | MiB/s at p50 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| repetitive | 2131 | yes | 2000 | 4.292µs | 4.667µs | 5µs | 474 |
| incompressible | 2071 | no | 2000 | 1.208µs | 1.417µs | 1.625µs | 1635 |
| repetitive | 16468 | yes | 2000 | 27.375µs | 30.791µs | 34.041µs | 574 |
| incompressible | 16421 | no | 2000 | 9.584µs | 9.709µs | 10.333µs | 1634 |
| repetitive | 65627 | yes | 1022 | 103.417µs | 109.5µs | 114.334µs | 605 |
| incompressible | 65561 | no | 1023 | 30.958µs | 32.25µs | 40.25µs | 2020 |
| repetitive | 262193 | yes | 255 | 410.584µs | 431.375µs | 460.75µs | 609 |
| incompressible | 262156 | no | 255 | 120.875µs | 123.167µs | 127.958µs | 2068 |
| repetitive | 1048628 | yes | 100 | 1.6195ms | 1.653458ms | 1.676041ms | 618 |
| incompressible | 1048606 | no | 100 | 487.666µs | 500.833µs | 506.791µs | 2051 |

### deslag-proxy request latency (273190 byte request, concurrency 1, warmup 20, samples 500)

| path | upstream bytes | p50 | p95 | p99 |
| --- | --- | --- | --- | --- |
| direct to mock | 273190 | 81.75µs | 98.333µs | 112.583µs |
| proxy, transparent | 273190 | 101.708µs | 124.792µs | 141.5µs |
| proxy, --minify-json | 244704 | 1.244042ms | 1.403667ms | 1.614917ms |

