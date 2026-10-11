#!/usr/bin/env bash
# Usage: package.sh <version> <target>
# Packs target/<target>/release/deslag with the licenses and README into
# dist/deslag-<version>-<target>.tar.gz under a deslag-<version>-<target>/ top dir.
set -euo pipefail
version=${1:?usage: package.sh <version> <target>}
target=${2:?usage: package.sh <version> <target>}
name=deslag-$version-$target
bin=target/$target/release/deslag

if [ ! -x "$bin" ]; then
  echo "error: $bin not found; build it with cargo build --release --locked --target $target -p deslag-cli" >&2
  exit 1
fi
stage=$(mktemp -d)
trap 'command rm -rf "$stage"' EXIT
mkdir "$stage/$name"
command cp -f "$bin" LICENSE-MIT LICENSE-APACHE README.md "$stage/$name/"
mkdir -p dist
# COPYFILE_DISABLE keeps macOS tar from adding AppleDouble ._ entries.
COPYFILE_DISABLE=1 tar -C "$stage" -czf "dist/$name.tar.gz" "$name"
echo "dist/$name.tar.gz"
