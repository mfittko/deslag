#!/usr/bin/env bash
# Usage: verify-approval.sh <version> <owner/repo> [<since>]
# A stable version needs an issue comment by the operator (RELEASE_OPERATOR,
# default: the repo owner) that has a line reading exactly
# `approve release v<version>` (trimmed, case-insensitive) and was created
# after <since> (ISO-8601 UTC or epoch seconds; default: HEAD committer date).
# An edited comment (updated_at differs from created_at) does not count: a
# write-access collaborator can edit another user's comment. A comment on a
# pull request does not count.
# A prerelease version (a `-` before any `+` build metadata) needs no approval.
# Any lookup failure fails closed.
set -euo pipefail
version=${1:?usage: verify-approval.sh <version> <owner/repo> [<since>]}
repo=${2:?usage: verify-approval.sh <version> <owner/repo> [<since>]}
since=${3:-$(git log -1 --format=%ct HEAD)}
operator=${RELEASE_OPERATOR:-${repo%%/*}}

case ${version%%+*} in *-*)
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
# --paginate prints one array per page; -n with inputs reads them all.
# Each candidate line is "<issue number> <html_url>".
candidates=$(jq -nr --arg op "$operator" --arg phrase "approve release v$version" --argjson since "$since_epoch" '
  inputs[]
  | select((.user.login | ascii_downcase) == ($op | ascii_downcase))
  | select((.created_at | fromdateiso8601) > $since)
  | select(.updated_at == .created_at)
  | select(any(.body | gsub("\r"; "") | split("\n")[]; gsub("^\\s+|\\s+$"; "") | ascii_downcase == $phrase))
  | "\(.issue_url | split("/") | last) \(.html_url)"
' <<<"$comments")

# The issue-comments endpoint also returns pull request conversation comments.
# Only a comment on an issue counts.
match=
while read -r number url; do
  [ -n "$number" ] || continue
  if ! is_pr=$(gh api "repos/$repo/issues/$number" --jq 'has("pull_request")'); then
    echo "error: could not look up $repo#$number; refusing the release" >&2
    exit 1
  fi
  if [ "$is_pr" = false ]; then match=$url; break; fi
done <<<"$candidates"

if [ -z "$match" ]; then
  echo "error: no approval: $operator has not commented 'approve release v$version' on an issue of $repo after $since_iso" >&2
  exit 1
fi
echo "ok: release v$version approved by $operator: $match"
