#!/usr/bin/env bash
# Usage: check-versions.sh [<tag-or-version>]
# Prints the release version. Fails when the workspace Cargo.toml version and
# the Claude plugin manifest version differ, or when the optional argument
# (v-prefixed or bare) differs from them.
set -euo pipefail

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
if [ $# -ge 1 ] && [ "${1#v}" != "$cargo_version" ]; then
  echo "error: $1 does not match the version surfaces ($cargo_version)" >&2
  exit 1
fi
echo "$cargo_version"
