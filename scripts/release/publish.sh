#!/usr/bin/env bash
# Usage: publish.sh <version> <action> <asset>...
# Publishes the assets as the GitHub Release v<version>. <action> is the
# output of release-state.sh for the same asset list. `create`
# creates the release with release-notes.md as its notes, marked as a
# prerelease when the version has a `-` suffix. `upload` uploads every asset
# into the existing release. It never passes --clobber, so it never replaces
# an asset. `skip` changes nothing. A gh error fails the script.
set -euo pipefail
version=${1:?usage: publish.sh <version> <action> <asset>...}
action=${2:?usage: publish.sh <version> <action> <asset>...}
shift 2
tag=v$version
assets=("$@")
if [ ${#assets[@]} -eq 0 ]; then
  echo "error: no assets given" >&2
  exit 1
fi
# An unmatched glob such as dist/*.tar.gz arrives as a literal path.
for f in "${assets[@]}"; do
  [ -f "$f" ] || { echo "error: asset not found: $f" >&2; exit 1; }
done
case $action in
  skip)
    echo "release $tag already has every asset; nothing to publish" >&2
    ;;
  upload)
    gh release upload "$tag" "${assets[@]}"
    ;;
  create)
    flags=()
    case ${version%%+*} in *-*) flags+=(--prerelease) ;; esac
    gh release create "$tag" "${assets[@]}" \
      --verify-tag --title "$tag" --notes-file release-notes.md ${flags[@]+"${flags[@]}"}
    ;;
  *)
    echo "error: unknown action '$action' (expected create, upload or skip)" >&2
    exit 1
    ;;
esac
