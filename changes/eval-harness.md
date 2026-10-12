### Added

- A development-only `deslag-eval` harness runs paired read-back tasks over original and compressed tool results, with predeclared scoring and a non-inferiority criterion in `docs/benchmark-methodology.md`, a synthetic corpus with incompressible and evidence-drop controls in `tests/evaluations/`, an offline mock backend for CI, and a `claude` backend that needs `DESLAG_EVAL_LIVE=1` and a spending cap. Results and the pending live run are in `docs/benchmarks/evaluation.md` (#16)
