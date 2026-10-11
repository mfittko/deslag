#!/usr/bin/env bash
# Usage: verify-approval.sh <version> <owner/repo> [<since>]
# A stable version needs an issue comment by the operator (RELEASE_OPERATOR,
# default: the repo owner) that has a line reading exactly
# `approve release v<version>` (trimmed, case-insensitive) and was created
# after <since> (ISO-8601 UTC or epoch seconds; default: HEAD committer date).
# A prerelease version (one with a `-` suffix) needs no approval.
# Any lookup failure fails closed.
set -euo pipefail
version=${1:?usage: verify-approval.sh <version> <owner/repo> [<since>]}
repo=${2:?usage: verify-approval.sh <version> <owner/repo> [<since>]}
since=${3:-$(git log -1 --format=%ct HEAD)}
operator=${RELEASE_OPERATOR:-${repo%%/*}}

case $version in *-*)
  echo "notice: $version is a prerelease; no operator approval needed"
  exit 0
  ;;
esac

if ! since_epoch=$(jq -nr --arg s "$since" 'if ($s | test("^[0-9]+$")) then $s | tonumber else $s | fromdateiso8601 end' 2>/dev/null); then
  echo "error: since '$since' is neither epoch seconds nor ISO-8601 UTC (YYYY-MM-DDTHH:MM:SSZ)" >&2
  exit 1
fi
since_iso=$(jq -nr --argjson e "$since_epoch" '$e | todate')

if ! comments=$(gh api --paginate "repos/$repo/issues/comments?since=$since_iso&per_page=100"); then
  echo "error: could not list issue comments of $repo; refusing the release" >&2
  exit 1
fi
match=$(jq -r --arg op "$operator" --arg phrase "approve release v$version" --argjson since "$since_epoch" '
  .[]
  | select((.user.login | ascii_downcase) == ($op | ascii_downcase))
  | select((.created_at | fromdateiso8601) > $since)
  | select(any(.body | gsub("\r"; "") | split("\n")[]; gsub("^\\s+|\\s+$"; "") | ascii_downcase == $phrase))
  | .html_url
' <<<"$comments" | head -n 1)

if [ -z "$match" ]; then
  echo "error: no approval: $operator has not commented 'approve release v$version' on an issue of $repo after $since_iso" >&2
  exit 1
fi
echo "ok: release v$version approved by $operator: $match"
