#!/usr/bin/env bash
# Usage: PR_BODY=<text> check-changelog-fragment.sh <base-ref>
# A change under crates/ or plugins/ needs a non-empty changes/<slug>.md
# fragment in the same diff, or a PR body line that reads `changelog: none`.
set -euo pipefail
base=${1:?usage: check-changelog-fragment.sh <base-ref>}

if ! git diff --name-only "$base...HEAD" | grep -qE '^(crates|plugins)/'; then
  echo "ok: no changes under crates/ or plugins/"
  exit 0
fi
if printf '%s\n' "${PR_BODY:-}" | tr -d '\r' | grep -qiE '^[[:space:]]*changelog:[[:space:]]*none[[:space:]]*$'; then
  echo "ok: PR body opts out with 'changelog: none'"
  exit 0
fi
while IFS= read -r f; do
  case $f in changes/README.md | changes/*/*) continue ;; changes/*.md) ;; *) continue ;; esac
  if [ -f "$f" ] && [ ! -L "$f" ] && grep -q '[^[:space:]]' "$f"; then
    echo "ok: fragment $f"
    exit 0
  fi
done < <(git diff --name-only --diff-filter=AM "$base...HEAD")

echo "error: changes under crates/ or plugins/ need a non-empty changes/<slug>.md fragment" >&2
echo "       (see changes/README.md) or a PR body line 'changelog: none'" >&2
exit 1
