#!/usr/bin/env bash
# Runs every benchmark in release mode and prints a Markdown report with
# machine and commit metadata. Usage: benchmarks/run.sh > docs/benchmarks/baseline.md
# The runners measure only; they apply no pass/fail thresholds.
set -euo pipefail

cd "$(dirname "$0")/.."

if [ "$(uname -s)" = Darwin ]; then
  cpu=$(sysctl -n machdep.cpu.brand_string)
  cpus=$(sysctl -n hw.logicalcpu)
else
  cpu=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//')
  cpus=$(nproc)
fi
dirty=no
if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  dirty=yes
fi

cat <<EOF
# Benchmark baseline

PLAN §13.2 budgets remain provisional. This file records measurements only; release thresholds are not frozen here.

## Metadata

| Field | Value |
| --- | --- |
| Date (UTC) | $(date -u +%Y-%m-%dT%H:%M:%SZ) |
| Commit | $(git rev-parse HEAD) |
| Uncommitted tracked changes | $dirty |
| CPU | $cpu |
| Logical CPUs | $cpus |
| OS | $(uname -a) |
| cargo | $(cargo -V) |
| Profile | bench (release), default features |

\`rustc -Vv\`:

\`\`\`
$(rustc -Vv)
\`\`\`

## Results

EOF

# Only the benchmark binaries print to stdout; cargo build output goes to stderr.
cargo bench --locked --workspace --bench minify --bench hook --bench proxy 2>/dev/null
