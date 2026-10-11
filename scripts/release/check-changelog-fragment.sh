#!/usr/bin/env bash
# Usage: PR_BODY=<text> check-changelog-fragment.sh <base-ref>
# A change under crates/ or plugins/ needs a changes/<slug>.md fragment in the
# same diff, or a PR body line that reads `changelog: none`. Every added or
# modified fragment must pass the format rules bump.sh applies (lib.sh) and
# hold at least one entry.
set -euo pipefail
base=${1:?usage: check-changelog-fragment.sh <base-ref>}
# shellcheck source=scripts/release/lib.sh
. "$(dirname "$0")/lib.sh"

# --no-renames lists a renamed file under its new name as an addition.
changed() { git diff --no-renames --name-only "$@" "$base...HEAD"; }

# grep reads all input (no -q) so pipefail never sees a SIGPIPE from git.
if ! changed | grep -E '^(crates|plugins)/' >/dev/null; then
  echo "ok: no changes under crates/ or plugins/"
  exit 0
fi
if printf '%s\n' "${PR_BODY:-}" | tr -d '\r' | grep -iE '^[[:space:]]*changelog:[[:space:]]*none[[:space:]]*$' >/dev/null; then
  echo "ok: PR body opts out with 'changelog: none'"
  exit 0
fi
found=0 bad=0
while IFS= read -r f; do
  case $f in changes/README.md | changes/*/*) continue ;; changes/*.md) ;; *) continue ;; esac
  if [ ! -f "$f" ] || [ -L "$f" ]; then
    echo "error: $f: a fragment must be a regular file" >&2
    bad=1
  elif awk -v f="$f" "$fragment_awk"'
    FNR == 1 { sec = "Changed" }
    { take($0) }
    END { if (err) exit 1; if (!n) { printf "error: %s: no \"- \" entry\n", f > "/dev/stderr"; exit 1 } }
  ' "$f"; then
    echo "ok: fragment $f"
    found=1
  else
    bad=1
  fi
done < <(changed --diff-filter=AM)

if [ "$bad" = 1 ]; then
  echo "error: fix the fragment format; see changes/README.md" >&2
  exit 1
fi
[ "$found" = 0 ] || exit 0
echo "error: changes under crates/ or plugins/ need a changes/<slug>.md fragment" >&2
echo "       (see changes/README.md) or a PR body line 'changelog: none'" >&2
exit 1
