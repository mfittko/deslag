#!/usr/bin/env bash
# Usage: check-versions.sh [<tag-or-version>]
# Prints the release version. Fails when the workspace Cargo.toml version and
# the Claude plugin manifest version differ, when fuzz/Cargo.lock pins another
# deslag-core version, or when the optional argument
# (v-prefixed or bare) differs from them. Fails on a version that is no SemVer
# X.Y.Z or X.Y.Z-pre; build metadata (`+...`) is refused.
set -euo pipefail
export LC_ALL=C
# shellcheck source=scripts/release/lib.sh
. "$(dirname "$0")/lib.sh"

cargo_version=$(awk '
  /^\[/ { in_pkg = ($0 == "[workspace.package]") }
  in_pkg && /^version[[:space:]]*=/ { sub(/^[^"]*"/, ""); sub(/".*/, ""); print; exit }
' Cargo.toml)
plugin_version=$(jq -r '.version // empty' plugins/claude/.claude-plugin/plugin.json)

if [ -z "$cargo_version" ]; then
  echo "error: no version in [workspace.package] of Cargo.toml" >&2
  exit 1
fi
if [ "$cargo_version" != "$plugin_version" ]; then
  echo "error: version surfaces differ: Cargo.toml=$cargo_version plugin.json=${plugin_version:-<missing>}" >&2
  exit 1
fi
if ! [[ $cargo_version =~ $semver ]]; then
  echo "error: '$cargo_version' is not a SemVer version (X.Y.Z or X.Y.Z-pre, no build metadata)" >&2
  exit 1
fi
# The fuzz crate is its own workspace with its own lock file.
fuzz_version=$(awk '
  /^\[\[package\]\]/ { in_core = 0 }
  $0 == "name = \"deslag-core\"" { in_core = 1 }
  in_core && /^version = / { sub(/^version = "/, ""); sub(/"$/, ""); print; exit }
' fuzz/Cargo.lock)
if [ "$fuzz_version" != "$cargo_version" ]; then
  echo "error: fuzz/Cargo.lock pins deslag-core ${fuzz_version:-<missing>}, not $cargo_version; run cargo update --workspace --offline --manifest-path fuzz/Cargo.toml" >&2
  exit 1
fi
if [ $# -ge 1 ] && [ "${1#v}" != "$cargo_version" ]; then
  echo "error: $1 does not match the version surfaces ($cargo_version)" >&2
  exit 1
fi
echo "$cargo_version"
