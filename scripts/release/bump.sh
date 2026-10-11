#!/usr/bin/env bash
# Usage: bump.sh <version>
# Sets the workspace Cargo.toml and Claude plugin versions, refreshes
# Cargo.lock, folds changes/*.md fragments into `## Unreleased`, stamps that
# section as `## <version> - <UTC date>` under a fresh empty `## Unreleased`,
# and stages exactly those files. It never commits, tags or pushes.
# The checks run before the first write. Until Cargo.lock is refreshed, any
# failure restores the original files, so a refusal, or a failure up to and
# including `cargo update`, changes no file. A later failure (git add, rm,
# git rm) can leave the rewritten files in the tree.
set -euo pipefail
# Fragment order (the glob) and character classes must not depend on the locale.
export LC_ALL=C
version=${1:?usage: bump.sh <version>}
plugin=plugins/claude/.claude-plugin/plugin.json
files=(CHANGELOG.md Cargo.toml "$plugin" Cargo.lock)
# shellcheck source=scripts/release/lib.sh
. "$(dirname "$0")/lib.sh"

if ! [[ $version =~ $semver ]]; then
  echo "error: '$version' is not a SemVer version (X.Y.Z or X.Y.Z-pre)" >&2
  exit 1
fi
if awk -v h="## $version" '{ sub(/\r$/, "") } $0 == h || index($0, h " ") == 1 { found = 1 } END { exit !found }' CHANGELOG.md; then
  echo "error: CHANGELOG.md already has a '## $version' section" >&2
  exit 1
fi

fragments=()
for f in changes/*.md; do
  [ "$f" = changes/README.md ] || [ ! -f "$f" ] || fragments+=("$f")
done

work=$(mktemp -d)
restore=0
trap 'if [ "$restore" = 1 ]; then for f in "${files[@]}"; do command cp -f "$work/orig/${f##*/}" "$f"; done; fi; command rm -rf "$work"' EXIT

# Entries keep their order: existing Unreleased entries first, then fragments
# in file-name order. Entries before any `### ` line belong to Changed.
awk -v cl=CHANGELOG.md -v ver="$version" -v date="$(date -u +%Y-%m-%d)" "$fragment_awk"'
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
  END { if (!done) { print "error: Cargo.toml has no version line under an exact \"[workspace.package]\" header" > "/dev/stderr"; exit 1 } }
' Cargo.toml >"$work/Cargo.toml"
jq --arg v "$version" '.version = $v' "$plugin" >"$work/plugin.json"

mkdir "$work/orig"
for f in "${files[@]}"; do command cp -f "$f" "$work/orig/${f##*/}"; done
restore=1
command mv -f "$work/CHANGELOG.md" CHANGELOG.md
command mv -f "$work/Cargo.toml" Cargo.toml
command mv -f "$work/plugin.json" "$plugin"
cargo update --workspace --offline --quiet
restore=0

git add -- "${files[@]}"
if [ ${#fragments[@]} -gt 0 ]; then
  command rm -f -- "${fragments[@]}"
  git rm -q --cached --ignore-unmatch -- "${fragments[@]}"
fi
echo "staged release $version; review with git diff --cached, then commit"
