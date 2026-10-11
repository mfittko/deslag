#!/usr/bin/env bash
# Usage: extract-notes.sh <version|Unreleased> [<changelog>]
# Prints the body of the `## <version>` section, trimmed of leading and
# trailing blank lines. Fails when the section is missing or empty.
set -euo pipefail
version=${1:?usage: extract-notes.sh <version|Unreleased> [<changelog>]}

awk -v v="$version" '
  found && /^## / { exit }
  found { line[++n] = $0; if ($0 ~ /[^[:space:]]/) { if (!first) first = n; last = n }; next }
  $0 == "## " v || index($0, "## " v " ") == 1 { found = 1 }
  END {
    if (!found) { print "error: no '\''## " v "'\'' section in " FILENAME > "/dev/stderr"; exit 1 }
    if (!last) { print "error: the '\''## " v "'\'' section is empty" > "/dev/stderr"; exit 1 }
    for (i = first; i <= last; i++) print line[i]
  }
' "${2:-CHANGELOG.md}"
