#!/usr/bin/env bash
# Usage: checksums.sh <dir>
# Writes <dir>/SHA256SUMS for every *.tar.gz in <dir>, in `sha256sum -c` format.
set -euo pipefail
export LC_ALL=C
cd "${1:?usage: checksums.sh <dir>}"
if command -v sha256sum >/dev/null; then sum=(sha256sum); else sum=(shasum -a 256); fi
"${sum[@]}" ./*.tar.gz | sed 's| \./| |' >SHA256SUMS
cat SHA256SUMS
