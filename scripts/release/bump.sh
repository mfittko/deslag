#!/usr/bin/env bash
# Usage: bump.sh <version>
# Sets the workspace Cargo.toml and Claude plugin versions, refreshes
# Cargo.lock, folds changes/*.md fragments into `## Unreleased`, stamps that
# section as `## <version> - <UTC date>` under a fresh empty `## Unreleased`,
# and stages exactly those files. It never commits, tags or pushes.
# All checks run before the first write, so a refusal leaves the tree untouched.
set -euo pipefail
version=${1:?usage: bump.sh <version>}
plugin=plugins/claude/.claude-plugin/plugin.json
semver='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$'

if ! grep -qE "$semver" <<<"$version"; then
  echo "error: '$version' is not a SemVer version (X.Y.Z or X.Y.Z-pre)" >&2
  exit 1
fi
if "$(dirname "$0")/extract-notes.sh" "$version" >/dev/null 2>&1 || grep -qF "## $version " CHANGELOG.md; then
  echo "error: CHANGELOG.md already has a '## $version' section" >&2
  exit 1
fi

fragments=()
for f in changes/*.md; do
  [ "$f" = changes/README.md ] || [ ! -f "$f" ] || fragments+=("$f")
done

work=$(mktemp -d)
trap 'command rm -rf "$work"' EXIT

# Entries keep their order: existing Unreleased entries first, then fragments
# in file-name order. A line before any `### ` heading belongs to Changed.
awk -v cl=CHANGELOG.md -v ver="$version" -v date="$(date -u +%Y-%m-%d)" '
  function die(msg) { printf "error: %s: %s\n", FILENAME, msg > "/dev/stderr"; err = 1; exit 1 }
  function take(line,   s) {
    if (line !~ /[^[:space:]]/) return
    if (line ~ /^### /) {
      s = substr(line, 5); sub(/[[:space:]]+$/, "", s)
      if (!(s in known)) die("unknown section heading \"" line "\" (use Added, Changed or Fixed)")
      sec = s; return
    }
    if (line ~ /^#/) die("unexpected heading \"" line "\"")
    entries[sec] = entries[sec] line "\n"; n++
  }
  BEGIN { known["Added"]; known["Changed"]; known["Fixed"] }
  FNR == 1 { sec = "Changed" }
  FILENAME != cl { take($0); next }
  state == 0 && /^## Unreleased[[:space:]]*$/ { state = 1; next }
  state == 0 { pre = pre $0 "\n"; next }
  state == 1 && /^## / { state = 2 }
  state == 1 { take($0); next }
  { post = post $0 "\n" }
  END {
    if (err) exit 1
    if (!state) { print "error: CHANGELOG.md has no \"## Unreleased\" section" > "/dev/stderr"; exit 1 }
    if (!n) { print "error: nothing to release: ## Unreleased is empty and changes/ has no fragments" > "/dev/stderr"; exit 1 }
    printf "%s## Unreleased\n\n## %s - %s\n", pre, ver, date
    split("Added Changed Fixed", order, " ")
    for (i = 1; i <= 3; i++) if (entries[order[i]] != "") printf "\n### %s\n\n%s", order[i], entries[order[i]]
    if (post != "") printf "\n%s", post
  }
' CHANGELOG.md ${fragments[@]+"${fragments[@]}"} >"$work/CHANGELOG.md"

awk -v ver="$version" '
  /^\[/ { in_pkg = ($0 == "[workspace.package]") }
  in_pkg && !done && /^version[[:space:]]*=/ { $0 = "version = \"" ver "\""; done = 1 }
  { print }
' Cargo.toml >"$work/Cargo.toml"
jq --arg v "$version" '.version = $v' "$plugin" >"$work/plugin.json"

command mv -f "$work/CHANGELOG.md" CHANGELOG.md
command mv -f "$work/Cargo.toml" Cargo.toml
command mv -f "$work/plugin.json" "$plugin"
cargo update --workspace --offline --quiet

git add -- Cargo.toml Cargo.lock "$plugin" CHANGELOG.md
if [ ${#fragments[@]} -gt 0 ]; then
  command rm -f -- "${fragments[@]}"
  git rm -q --cached --ignore-unmatch -- "${fragments[@]}"
fi
echo "staged release $version; review with git diff --cached, then commit"
