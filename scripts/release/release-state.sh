#!/usr/bin/env bash
# Usage: release-state.sh <tag> <asset>...
# Prints the publish action for <tag>. `create`: the release does not exist.
# `upload`: the release has none of the assets. `skip`: it has every asset.
# Fails on a partial release. Fails on any `gh release view` error other than
# "release not found", so a transient API or auth error never reads as absent.
set -euo pipefail
tag=${1:?usage: release-state.sh <tag> <asset>...}; shift
err=$(mktemp)
trap 'command rm -f "$err"' EXIT
if ! existing=$(gh release view "$tag" --json assets --jq '.assets[].name' 2>"$err"); then
  if grep -qxF 'release not found' "$err"; then echo create; exit 0; fi
  echo "::error::could not look up release $tag: $(cat "$err")" >&2
  exit 1
fi
missing=()
for f in "$@"; do
  grep -qxF "${f##*/}" <<<"$existing" || missing+=("${f##*/}")
done
if [ ${#missing[@]} -eq 0 ]; then
  echo "release $tag already has every asset" >&2
  echo skip
elif [ ${#missing[@]} -eq $# ]; then
  echo upload
else
  echo "::error::release $tag is partial (missing: ${missing[*]}); rebuilt archives would not match it. Run 'gh release delete $tag' and re-run this workflow." >&2
  exit 1
fi
