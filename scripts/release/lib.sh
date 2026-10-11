# shellcheck shell=bash
# Sourced by bump.sh, check-changelog-fragment.sh and check-versions.sh. It
# holds the one copy of the SemVer pattern in $semver and of the fragment
# format rules, as awk functions in $fragment_awk. $semver refuses build
# metadata (`+...`), so a `-` in a valid version always marks a prerelease.
# The sourcing scripts export LC_ALL=C, so the character classes and ranges
# below match the same way in every locale.
#
# take(line) files one line of a fragment or of `## Unreleased`. A blank line
# is skipped. `### Added`, `### Changed` or `### Fixed` switches the section.
# A `- <text>` line is an entry under the current section. Any other line
# calls die(). Callers set sec = "Changed" at the start of each file and read
# entries[], n (the entry count) and err.
# shellcheck disable=SC2034 # read by the sourcing scripts
semver='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$'
# shellcheck disable=SC2034 # read by the sourcing scripts
fragment_awk='
  function die(msg) { printf "error: %s: %s\n", FILENAME, msg > "/dev/stderr"; err = 1; exit 1 }
  function take(line,   s) {
    sub(/\r$/, "", line)
    if (line !~ /[^[:space:]]/) return
    if (line ~ /^### /) {
      s = substr(line, 5); sub(/[[:space:]]+$/, "", s)
      if (!(s in known)) die("unknown section heading \"" line "\" (use Added, Changed or Fixed)")
      sec = s; return
    }
    if (line !~ /^- [^[:space:]]/) die("expected a \"### Added|Changed|Fixed\" line or a \"- \" entry, got \"" line "\"")
    entries[sec] = entries[sec] line "\n"; n++
  }
  BEGIN { known["Added"]; known["Changed"]; known["Fixed"] }
'
