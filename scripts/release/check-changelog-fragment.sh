#!/usr/bin/env bash
# Usage: PR_BODY=<text> check-changelog-fragment.sh <base-ref>
# A change under crates/ or plugins/ needs a changes/<slug>.md fragment in the
# same diff, or a PR body line `changelog: none` (trimmed, case-insensitive). Every added or
# modified fragment must pass the format rules bump.sh applies (lib.sh) and
# hold at least one entry.
set -euo pipefail
export LC_ALL=C
base=${1:?usage: check-changelog-fragment.sh <base-ref>}
# shellcheck source=scripts/release/lib.sh
. "$(dirname "$0")/lib.sh"

# -z lists raw, NUL-terminated names: git C-quotes nothing, so a name with a
# quote, a backslash, a control character or a non-ASCII byte reaches the
# filters unchanged.
# --no-renames lists a renamed file under its new name as an addition.
# Both lists are captured up front: under set -e a git failure (for example an
# unresolvable base) aborts the check before any "ok".
lists=$(mktemp -d)
trap 'command rm -rf "$lists"' EXIT
changed() { git diff -z --no-renames --name-only "$@" "$base...HEAD"; }
changed >"$lists/files"
changed --diff-filter=AM >"$lists/fragments"

# Validation runs before the early exits below, so an untouched or opted-out
# PR cannot land a malformed fragment.
found=0 bad=0
while IFS= read -r -d '' f; do
  case $f in changes/README.md | changes/*/*) continue ;; changes/*.md) ;; *) continue ;; esac
  # bump.sh globs changes/*.md, and a bash glob skips dotfiles.
  case $f in changes/.*)
    echo "error: $f: a fragment name must not start with a dot" >&2
    bad=1
    continue
    ;;
  esac
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
done <"$lists/fragments"

if [ "$bad" = 1 ]; then
  echo "error: fix the fragment format; see changes/README.md" >&2
  exit 1
fi

touched=0
while IFS= read -r -d '' f; do
  case $f in crates/* | plugins/*) touched=1 ;; esac
done <"$lists/files"
if [ "$touched" = 0 ]; then
  echo "ok: no changes under crates/ or plugins/"
  exit 0
fi
if printf '%s\n' "${PR_BODY:-}" | tr -d '\r' | grep -iE '^[[:space:]]*changelog:[[:space:]]*none[[:space:]]*$' >/dev/null; then
  echo "ok: PR body opts out with 'changelog: none'"
  exit 0
fi
[ "$found" = 0 ] || exit 0
echo "error: changes under crates/ or plugins/ need a changes/<slug>.md fragment" >&2
echo "       (see changes/README.md) or a PR body line 'changelog: none'" >&2
exit 1
