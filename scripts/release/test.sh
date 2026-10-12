#!/usr/bin/env bash
# Self-contained tests for scripts/release/*.sh. Each case runs in a throwaway
# git repository; verify-approval.sh runs against a fake `gh` on PATH.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'command rm -rf "$tmp"' EXIT
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid
export GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export LC_ALL=C
unset RELEASE_OPERATOR FAKE_GH_FAIL
failures=0

ok() { echo "ok - $1"; }
not_ok() { echo "not ok - $1"; printf '    %s\n' "${2//$'\n'/$'\n'    }"; failures=$((failures + 1)); }

# expect_ok <desc> <dir> <cmd...>
expect_ok() {
  local desc=$1 dir=$2 out; shift 2
  if out=$(cd "$dir" && "$@" 2>&1); then ok "$desc"; else not_ok "$desc" "$out"; fi
}

# expect_fail <desc> <pattern> <dir> <cmd...>: fails and stderr/stdout matches pattern.
expect_fail() {
  local desc=$1 pattern=$2 dir=$3 out; shift 3
  if out=$(cd "$dir" && "$@" 2>&1); then not_ok "$desc (unexpected success)" "$out"
  elif grep -qiE -- "$pattern" <<<"$out"; then ok "$desc"
  else not_ok "$desc (output did not match /$pattern/)" "$out"; fi
}

# expect_eq <desc> <expected> <actual>
expect_eq() {
  if [ "$2" = "$3" ]; then ok "$1"; else not_ok "$1" "$(printf 'expected:\n%s\nactual:\n%s' "$2" "$3")"; fi
}

new_repo() {
  local dir=$tmp/$1
  mkdir -p "$dir/crates/a/src" "$dir/fuzz/src" "$dir/plugins/claude/.claude-plugin" "$dir/changes"
  cat >"$dir/Cargo.toml" <<'EOF'
[workspace]
resolver = "2"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2021"
EOF
  cat >"$dir/crates/a/Cargo.toml" <<'EOF'
[package]
name = "deslag-core"
version.workspace = true
edition.workspace = true
EOF
  : >"$dir/crates/a/src/lib.rs"
  # The fuzz crate is its own workspace with its own lock file.
  cat >"$dir/fuzz/Cargo.toml" <<'EOF'
[package]
name = "fuzz"
version = "0.0.0"
edition = "2021"

[workspace]

[dependencies]
deslag-core = { path = "../crates/a" }
EOF
  : >"$dir/fuzz/src/lib.rs"
  printf '{\n  "name": "deslag",\n  "version": "0.1.0"\n}\n' >"$dir/plugins/claude/.claude-plugin/plugin.json"
  printf '# Changelog\n\n## Unreleased\n\n## 0.0.1 - 2020-01-01\n\n- First.\n' >"$dir/CHANGELOG.md"
  echo "# Fragments" >"$dir/changes/README.md"
  echo "# readme" >"$dir/README.md"
  (cd "$dir" && git init -q -b main && cargo generate-lockfile --offline -q &&
    cargo generate-lockfile --offline -q --manifest-path fuzz/Cargo.toml && git add -A && git commit -qm base)
  echo "$dir"
}

# --- check-versions.sh -------------------------------------------------------
r=$(new_repo versions)
expect_eq "check-versions prints the shared version" "0.1.0" "$(cd "$r" && "$here/check-versions.sh")"
expect_ok "check-versions accepts a matching tag" "$r" "$here/check-versions.sh" v0.1.0
expect_ok "check-versions accepts a matching bare version" "$r" "$here/check-versions.sh" 0.1.0
expect_fail "check-versions rejects a tag that differs" "does not match" "$r" "$here/check-versions.sh" v0.2.0
printf '{\n  "version": "0.1.1"\n}\n' >"$r/plugins/claude/.claude-plugin/plugin.json"
expect_fail "check-versions rejects mismatched surfaces" "differ" "$r" "$here/check-versions.sh"
printf '{\n  "version": "0.1.0"\n}\n' >"$r/plugins/claude/.claude-plugin/plugin.json"
# set_fuzz_lock <version>: pin deslag-core in fuzz/Cargo.lock to <version>.
set_fuzz_lock() {
  awk -v v="$1" 'core && /^version = / { $0 = "version = \"" v "\""; core = 0 } $0 == "name = \"deslag-core\"" { core = 1 } { print }' \
    "$r/fuzz/Cargo.lock" >"$r/fuzz/Cargo.lock.new" && command mv -f "$r/fuzz/Cargo.lock.new" "$r/fuzz/Cargo.lock"
}
set_fuzz_lock 0.0.9
expect_fail "check-versions rejects a fuzz/Cargo.lock that pins another deslag-core" "fuzz/Cargo.lock pins deslag-core 0.0.9" "$r" "$here/check-versions.sh"
set_fuzz_lock 0.1.0
expect_ok "check-versions accepts a matching fuzz/Cargo.lock" "$r" "$here/check-versions.sh"
command mv -f "$r/fuzz/Cargo.lock" "$r/fuzz/Cargo.lock.bak"
expect_fail "check-versions fails without fuzz/Cargo.lock" "fuzz/Cargo.lock is missing" "$r" "$here/check-versions.sh"
command mv -f "$r/fuzz/Cargo.lock.bak" "$r/fuzz/Cargo.lock"
set_version() {
  sed "s/^version = .*/version = \"$1\"/" "$r/Cargo.toml" >"$r/Cargo.toml.new" && command mv -f "$r/Cargo.toml.new" "$r/Cargo.toml"
  printf '{\n  "version": "%s"\n}\n' "$1" >"$r/plugins/claude/.claude-plugin/plugin.json"
  set_fuzz_lock "$1"
}
for v in 0.1.0-alpha.1 1.0.0-rc.1 1.0.0; do
  set_version "$v"
  expect_ok "check-versions accepts $v" "$r" "$here/check-versions.sh" "v$v"
done
for v in 1.0.0+build 1.0.0+build-1; do
  set_version "$v"
  expect_fail "check-versions refuses build metadata ($v)" "not a semver" "$r" "$here/check-versions.sh" "v$v"
done
for v in 1.0.0-0 1.0.0-10 1.0.0-0a 1.0.0-rc.01a 1.0.0--1; do
  set_version "$v"
  expect_ok "check-versions accepts prerelease $v" "$r" "$here/check-versions.sh" "v$v"
done
for v in 1.0.0-01 1.0.0-rc.00; do
  set_version "$v"
  expect_fail "check-versions refuses a leading-zero numeric prerelease ($v)" "not a semver" "$r" "$here/check-versions.sh" "v$v"
done

# --- check-changelog-fragment.sh --------------------------------------------
r=$(new_repo fragments)
base=$(git -C "$r" rev-parse HEAD)
branch() { git -C "$r" checkout -q -B "$1" "$base"; }
commit() { git -C "$r" add -A && git -C "$r" commit -qm "$1"; }
check() { PR_BODY=$1 "$here/check-changelog-fragment.sh" "$base"; }

branch docs-only; echo more >>"$r/README.md"; commit docs
expect_ok "fragment check passes a change outside crates/ and plugins/" "$r" check ""

branch docs-good-fragment; echo more >>"$r/README.md"; printf -- '- Doc.\n' >"$r/changes/x.md"; commit docs
expect_ok "fragment check passes a docs-only change with a valid fragment" "$r" check ""

branch docs-bad-fragment; echo more >>"$r/README.md"; printf 'Just text.\n' >"$r/changes/x.md"; commit docs
expect_fail "fragment check rejects a malformed fragment in a docs-only change" "expected a" "$r" check ""

branch docs-dot-fragment; echo more >>"$r/README.md"; printf -- '- Doc.\n' >"$r/changes/.x.md"; commit docs
expect_fail "fragment check rejects a dot-prefixed fragment in a docs-only change" "must not start with a dot" "$r" check ""

branch optout-bad-fragment; echo "// o" >>"$r/crates/a/src/lib.rs"; printf 'Just text.\n' >"$r/changes/x.md"; commit bad
expect_fail "fragment check rejects a malformed fragment despite 'changelog: none'" "expected a" "$r" check "changelog: none"

branch release-base; printf -- '- Old.\n' >"$r/changes/old.md"; commit old
release_base=$(git -C "$r" rev-parse HEAD)
git -C "$r" rm -q changes/old.md; echo " " >>"$r/plugins/claude/.claude-plugin/plugin.json"; commit release
expect_ok "fragment check passes a release that deletes fragments with 'changelog: none'" "$r" env PR_BODY='changelog: none' "$here/check-changelog-fragment.sh" "$release_base"

branch no-fragment; echo "// x" >>"$r/crates/a/src/lib.rs"; commit code
expect_fail "fragment check fails crates/ change without fragment" "need a changes/<slug>.md" "$r" check ""
expect_ok "fragment check accepts 'changelog: none'" "$r" check "Summary"$'\n'"  Changelog: None  "$'\r'
expect_fail "fragment check rejects 'changelog: none' inside a sentence" "need a changes/<slug>.md" "$r" check "no changelog: none needed"

branch plugin; echo " " >>"$r/plugins/claude/.claude-plugin/plugin.json"; commit plugin
expect_fail "fragment check fails plugins/ change without fragment" "need a changes/<slug>.md" "$r" check ""

branch with-fragment; echo "// y" >>"$r/crates/a/src/lib.rs"; printf -- '- Thing.\n' >"$r/changes/thing.md"; commit frag
expect_ok "fragment check accepts a non-empty fragment" "$r" check ""

branch non-ascii-code; echo "// x" >"$r/crates/a/src/café.rs"; commit code
expect_fail "fragment check fails a non-ASCII crates/ path without fragment" "need a changes/<slug>.md" "$r" check ""

branch non-ascii-fragment; echo "// x" >"$r/crates/a/src/café.rs"; printf -- '- Thing.\n' >"$r/changes/café.md"; commit frag
expect_ok "fragment check counts a non-ASCII fragment name" "$r" check ""

n=0
for name in 'x"y.rs' $'x\ty.rs' 'x\y.rs'; do
  n=$((n + 1)); branch "quoted-$n"; echo "// x" >"$r/crates/a/src/$name"; commit code
  expect_fail "fragment check fails a git-quoted crates/ path without fragment (${name//$'\t'/<TAB>})" "need a changes/<slug>.md" "$r" check ""
done

branch empty-fragment; echo "// z" >>"$r/crates/a/src/lib.rs"; printf '\n  \n' >"$r/changes/empty.md"; commit empty
expect_fail "fragment check rejects an empty fragment" 'no "- " entry' "$r" check ""

branch readme-only; echo "// w" >>"$r/crates/a/src/lib.rs"; echo more >>"$r/changes/README.md"; commit readme
expect_fail "fragment check ignores changes/README.md" "need a changes/<slug>.md" "$r" check ""

branch bad-fragment; echo "// v" >>"$r/crates/a/src/lib.rs"; printf 'Just text.\n' >"$r/changes/bad.md"; commit bad
expect_fail "fragment check rejects a line that is no entry" "expected a" "$r" check ""

branch bad-heading; echo "// u" >>"$r/crates/a/src/lib.rs"; printf -- '### Removed\n- Gone.\n' >"$r/changes/bad.md"; commit bad
expect_fail "fragment check rejects an unknown section heading" "unknown section" "$r" check ""

branch one-bad; echo "// t" >>"$r/crates/a/src/lib.rs"; printf -- '- Fine.\n' >"$r/changes/good.md"; printf '## Heading\n- X.\n' >"$r/changes/bad.md"; commit bad
expect_fail "fragment check rejects a malformed fragment next to a valid one" "expected a" "$r" check ""

branch crlf-fragment; echo "// s" >>"$r/crates/a/src/lib.rs"; printf -- '### Fixed\r\n- Fine.\r\n' >"$r/changes/crlf.md"; commit crlf
expect_ok "fragment check accepts a CRLF fragment" "$r" check ""

branch symlink-fragment; echo "// q" >>"$r/crates/a/src/lib.rs"; ln -s ../README.md "$r/changes/link.md"; commit link
expect_fail "fragment check refuses a symlinked fragment" "regular file" "$r" check ""

branch dot-fragment; echo "// p" >>"$r/crates/a/src/lib.rs"; printf -- '- Fix.\n' >"$r/changes/.fix.md"; commit dot
expect_fail "fragment check rejects a dot-prefixed fragment name" "must not start with a dot" "$r" check ""

branch rename-base; printf -- '- Old.\n' >"$r/changes/old.md"; commit old
rename_base=$(git -C "$r" rev-parse HEAD)
git -C "$r" mv changes/old.md changes/new.md; echo "// r" >>"$r/crates/a/src/lib.rs"; commit rename
expect_ok "fragment check counts a renamed fragment" "$r" env PR_BODY= "$here/check-changelog-fragment.sh" "$rename_base"

branch bad-base; echo "// q" >>"$r/crates/a/src/lib.rs"; commit code
expect_fail "fragment check fails closed on an unresolvable base" "deadbeef" "$r" env PR_BODY= "$here/check-changelog-fragment.sh" deadbeef
if out=$(cd "$r" && PR_BODY='' "$here/check-changelog-fragment.sh" deadbeef 2>&1) || [[ $out == *"ok: no changes"* ]]; then
  not_ok "fragment check never reports 'ok: no changes' for an unresolvable base" "$out"
else ok "fragment check never reports 'ok: no changes' for an unresolvable base"; fi

# --- bump.sh -----------------------------------------------------------------
r=$(new_repo bump-empty)
expect_fail "bump rejects a non-SemVer version" "semver" "$r" "$here/bump.sh" 1.2
expect_fail "bump rejects a version with an embedded newline" "semver" "$r" "$here/bump.sh" $'1.0.0\nx'
expect_fail "bump rejects build metadata" "semver" "$r" "$here/bump.sh" 1.0.0+build-1
expect_fail "bump rejects a leading-zero numeric prerelease" "semver" "$r" "$here/bump.sh" 1.0.0-01
expect_fail "bump refuses an empty release" "nothing to release" "$r" "$here/bump.sh" 0.2.0
expect_eq "bump leaves the tree untouched on refusal" "" "$(git -C "$r" status --porcelain)"

r=$(new_repo bump)
printf '# Changelog\n\nIntro.\n\n## Unreleased\n\n- Loose entry.\n\n### Fixed\n\n- Old fix.\n\n## 0.0.1 - 2020-01-01\n\n- First.\n' >"$r/CHANGELOG.md"
printf -- '### Added\n\n- New feature.\n' >"$r/changes/feature.md"
printf -- '- Tweak.\n' >"$r/changes/tweak.md"
printf -- '### Fixed\n- New fix.\n' >"$r/changes/zfix.md"
git -C "$r" add -A && git -C "$r" commit -qm pending
head_before=$(git -C "$r" rev-parse HEAD)
# A run across UTC midnight may stamp the date from before the bump.
day_before=$(date -u +%Y-%m-%d)
expect_ok "bump succeeds with fragments" "$r" "$here/bump.sh" 0.2.0-alpha.1
expect_eq "bump sets both version surfaces" "0.2.0-alpha.1" "$(cd "$r" && "$here/check-versions.sh")"
expect_eq "bump refreshes Cargo.lock" "1" "$(grep -c '^version = "0.2.0-alpha.1"' "$r/Cargo.lock")"
expect_eq "bump refreshes fuzz/Cargo.lock" "1" "$(grep -c '^version = "0.2.0-alpha.1"' "$r/fuzz/Cargo.lock")"
today=$(date -u +%Y-%m-%d)
! grep -qxF "## 0.2.0-alpha.1 - $day_before" "$r/CHANGELOG.md" || today=$day_before
expected="# Changelog

Intro.

## Unreleased

## 0.2.0-alpha.1 - $today

### Added

- New feature.

### Changed

- Loose entry.
- Tweak.

### Fixed

- Old fix.
- New fix.

## 0.0.1 - 2020-01-01

- First.
."
expect_eq "bump assembles fragments and stamps the changelog" "$expected" "$(cat "$r/CHANGELOG.md"; echo .)"
expect_eq "bump stages exactly the release files" \
  "CHANGELOG.md Cargo.lock Cargo.toml changes/feature.md changes/tweak.md changes/zfix.md fuzz/Cargo.lock plugins/claude/.claude-plugin/plugin.json" \
  "$(git -C "$r" diff --cached --name-only | sort | tr '\n' ' ' | sed 's/ $//')"
expect_eq "bump leaves no unstaged changes" "" "$(git -C "$r" diff --name-only)"
expect_eq "bump keeps changes/README.md" "# Fragments" "$(cat "$r/changes/README.md")"
expect_eq "bump never commits" "$head_before" "$(git -C "$r" rev-parse HEAD)"
expect_eq "bump never tags" "" "$(git -C "$r" tag)"
git -C "$r" commit -qm release
printf -- '- Again.\n' >"$r/changes/again.md"
expect_fail "bump refuses a version already in the changelog" "already" "$r" "$here/bump.sh" 0.2.0-alpha.1

r=$(new_repo bump-bad)
printf -- '### Removed\n- Gone.\n' >"$r/changes/bad.md"
expect_fail "bump rejects an unknown section heading" "unknown section" "$r" "$here/bump.sh" 0.2.0
expect_eq "bump leaves the changelog untouched on error" "" "$(git -C "$r" status --porcelain -- CHANGELOG.md Cargo.toml)"
printf 'Not an entry.\n' >"$r/changes/bad.md"
expect_fail "bump rejects a line that is no entry" "expected a" "$r" "$here/bump.sh" 0.2.0

r=$(new_repo bump-undated)
printf '# Changelog\n\n## Unreleased\n\n- Pending.\n\n## 0.3.0\n\n## 0.0.1 - 2020-01-01\n\n- First.\n' >"$r/CHANGELOG.md"
expect_fail "bump refuses a version with an empty undated heading" "already" "$r" "$here/bump.sh" 0.3.0

r=$(new_repo bump-header)
printf -- '- Change.\n' >"$r/changes/change.md"
sed 's/^\[workspace\.package\]$/[workspace.package] # shared/' "$r/Cargo.toml" >"$r/Cargo.toml.new" && command mv -f "$r/Cargo.toml.new" "$r/Cargo.toml"
git -C "$r" add -A && git -C "$r" commit -qm pending
expect_fail "bump refuses a Cargo.toml it cannot rewrite" "workspace.package" "$r" "$here/bump.sh" 0.2.0
expect_eq "bump leaves the tree untouched when Cargo.toml is not rewritten" "" "$(git -C "$r" status --porcelain)"

r=$(new_repo bump-cargo-fail)
printf -- '- Change.\n' >"$r/changes/change.md"
git -C "$r" add -A && git -C "$r" commit -qm pending
mkdir -p "$tmp/fakecargo"
printf '#!/bin/sh\necho "cargo: simulated failure" >&2\nexit 101\n' >"$tmp/fakecargo/cargo"
chmod +x "$tmp/fakecargo/cargo"
expect_fail "bump fails when cargo update fails" "simulated failure" "$r" env PATH="$tmp/fakecargo:$PATH" "$here/bump.sh" 0.2.0
expect_eq "bump restores every file when cargo update fails" "" "$(git -C "$r" status --porcelain)"
# The root lock update succeeds, then the fuzz lock update fails.
mkdir -p "$tmp/fakecargo-fuzz"
cat >"$tmp/fakecargo-fuzz/cargo" <<EOF
#!/bin/sh
case " \$* " in *" fuzz/Cargo.toml "*) echo "cargo: simulated fuzz failure" >&2; exit 101 ;; esac
exec "$(command -v cargo)" "\$@"
EOF
chmod +x "$tmp/fakecargo-fuzz/cargo"
expect_fail "bump fails when the fuzz lock update fails" "simulated fuzz failure" "$r" env PATH="$tmp/fakecargo-fuzz:$PATH" "$here/bump.sh" 0.2.0
expect_eq "bump restores both lock files when the fuzz lock update fails" "" "$(git -C "$r" status --porcelain)"
expect_fail "bump names the fuzz fetch prerequisite when the fuzz lock update fails" "cargo fetch --manifest-path fuzz/Cargo.toml" "$r" env PATH="$tmp/fakecargo-fuzz:$PATH" "$here/bump.sh" 0.2.0

# --- extract-notes.sh --------------------------------------------------------
r=$tmp/notes; mkdir -p "$r"
printf '# Changelog\n\n## Unreleased\n\n- Pending.\n\n## 1.0.0 - 2026-01-02\n\n### Added\n\n- One.\n\n## 1.0.0-rc.1 - 2026-01-01\n\n## 0.9.0\n- Old.\n' >"$r/CHANGELOG.md"
expect_eq "extract-notes prints the trimmed section body" "### Added

- One." "$(cd "$r" && "$here/extract-notes.sh" 1.0.0)"
expect_eq "extract-notes reads a heading without a date" "- Old." "$(cd "$r" && "$here/extract-notes.sh" 0.9.0)"
expect_eq "extract-notes reads Unreleased" "- Pending." "$(cd "$r" && "$here/extract-notes.sh" Unreleased)"
expect_fail "extract-notes fails on a missing section" "no '## 2.0.0' section" "$r" "$here/extract-notes.sh" 2.0.0
expect_fail "extract-notes fails on an empty section" "empty" "$r" "$here/extract-notes.sh" 1.0.0-rc.1
expect_fail "extract-notes does not match a version prefix" "no '## 1.0' section" "$r" "$here/extract-notes.sh" 1.0
printf '## Unreleased\r\n\r\n## 2.0.0\r\n\r\n- Two.\r\n\r\n## 1.0.0\r\n' >"$r/CRLF.md"
expect_eq "extract-notes reads a CRLF changelog" "- Two." "$(cd "$r" && "$here/extract-notes.sh" 2.0.0 CRLF.md)"

# --- verify-approval.sh ------------------------------------------------------
r=$(new_repo approval)
mkdir -p "$tmp/bin"
cat >"$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
[ -z "${FAKE_GH_FAIL:-}" ] || { echo "gh: HTTP 403" >&2; exit 1; }
# An issue lookup answers has("pull_request"): true for numbers in FAKE_GH_PRS.
if [[ $2 =~ /issues/([0-9]+)$ ]]; then
  [ -z "${FAKE_GH_ISSUE_FAIL:-}" ] || { echo "gh: HTTP 404" >&2; exit 1; }
  case " ${FAKE_GH_PRS:-} " in *" ${BASH_REMATCH[1]} "*) echo true ;; *) echo false ;; esac
  exit 0
fi
cat "$FAKE_GH_JSON"
EOF
chmod +x "$tmp/bin/gh"
export FAKE_GH_JSON=$tmp/comments.json
since=2026-01-01T00:00:00Z
comments() { printf '%s\n' "$@" >"$FAKE_GH_JSON"; }
# c <login> <created_at> <body> [<updated_at>] [<issue>]: updated_at defaults to
# created_at (an unedited comment); issue defaults to 1.
c() { printf '{"user":{"login":"%s"},"created_at":"%s","updated_at":"%s","body":"%s","html_url":"u%s","issue_url":"https://api.github.com/repos/mfittko/deslag/issues/%s"}' "$1" "$2" "${4:-$2}" "$3" "${5:-1}" "${5:-1}"; }
approve() { PATH="$tmp/bin:$PATH" "$here/verify-approval.sh" "$1" mfittko/deslag "$since"; }

FAKE_GH_FAIL=1 expect_ok "approval skips prereleases without calling gh" "$r" approve 1.0.0-rc.1
FAKE_GH_FAIL=1 expect_ok "approval skips an alpha prerelease without calling gh" "$r" approve 0.1.0-alpha.1
FAKE_GH_FAIL=1 expect_fail "approval checks a stable version" "could not list" "$r" approve 1.0.0
FAKE_GH_FAIL=1 expect_fail "approval treats hyphenated build metadata as stable" "could not list" "$r" approve 1.0.0+build-1
comments "[$(c MFittko 2026-01-02T00:00:00Z 'LGTM.\r\n  Approve Release V1.0.0 \r\n')]"
expect_ok "approval accepts the owner's exact line" "$r" approve 1.0.0
comments "[$(c someone 2026-01-02T00:00:00Z 'approve release v1.0.0')]"
expect_fail "approval rejects a non-owner comment" "no approval" "$r" approve 1.0.0
RELEASE_OPERATOR=someone expect_ok "approval honors RELEASE_OPERATOR" "$r" approve 1.0.0
comments "[$(c mfittko 2026-01-02T00:00:00Z 'approve release v1.0.0' 2026-01-02T00:00:01Z)]"
expect_fail "approval rejects an edited comment" "no approval" "$r" approve 1.0.0
comments "[$(c mfittko 2025-12-31T23:59:59Z 'approve release v1.0.0')]"
expect_fail "approval rejects a comment older than the release commit" "no approval" "$r" approve 1.0.0
comments "[$(c mfittko 2026-01-02T00:00:00Z 'I do not approve release v1.0.0')]"
expect_fail "approval rejects the phrase inside a sentence" "no approval" "$r" approve 1.0.0
comments "[$(c mfittko 2026-01-02T00:00:00Z 'approve release v1.0.1')]"
expect_fail "approval rejects another version" "no approval" "$r" approve 1.0.0
comments "[$(c x 2026-01-02T00:00:00Z a)]" "[$(c mfittko 2026-01-03T00:00:00Z 'approve release v1.0.0')]"
expect_ok "approval reads every paginated page" "$r" approve 1.0.0
FAKE_GH_FAIL=1 expect_fail "approval fails closed when gh fails" "could not list" "$r" approve 1.0.0
comments "[$(c mfittko 2026-01-02T00:00:00Z 'approve release v1.0.0' '' 7)]"
FAKE_GH_PRS=7 expect_fail "approval rejects a comment on a pull request" "no approval" "$r" approve 1.0.0
comments "[$(c mfittko 2026-01-02T00:00:00Z 'approve release v1.0.0' '' 7),$(c mfittko 2026-01-03T00:00:00Z 'approve release v1.0.0' '' 8)]"
FAKE_GH_PRS=7 expect_ok "approval skips a pull request comment and accepts an issue comment" "$r" approve 1.0.0
FAKE_GH_ISSUE_FAIL=1 expect_fail "approval fails closed when the issue lookup fails" "could not look up" "$r" approve 1.0.0
expect_fail "approval rejects a malformed since" "since" "$r" "$here/verify-approval.sh" 1.0.0 mfittko/deslag yesterday
# release.yml passes no <since>: the HEAD committer date (epoch seconds) is the cutoff.
approve_default() { PATH="$tmp/bin:$PATH" "$here/verify-approval.sh" "$1" mfittko/deslag; }
comments "[$(c mfittko 2099-01-01T00:00:00Z 'approve release v1.0.0')]"
expect_ok "approval by default accepts a comment after the HEAD commit" "$r" approve_default 1.0.0
comments "[$(c mfittko 2020-01-01T00:00:00Z 'approve release v1.0.0')]"
expect_fail "approval by default rejects a comment before the HEAD commit" "no approval" "$r" approve_default 1.0.0

# --- package.sh + checksums.sh -----------------------------------------------
r=$tmp/package; mkdir -p "$r/target/x-test/release"
printf '#!/bin/sh\n' >"$r/target/x-test/release/deslag"; chmod +x "$r/target/x-test/release/deslag"
for f in LICENSE-MIT LICENSE-APACHE README.md; do echo "$f" >"$r/$f"; done
expect_ok "package builds an archive" "$r" "$here/package.sh" 1.0.0 x-test
expect_eq "package archive layout" \
  "deslag-1.0.0-x-test/ deslag-1.0.0-x-test/LICENSE-APACHE deslag-1.0.0-x-test/LICENSE-MIT deslag-1.0.0-x-test/README.md deslag-1.0.0-x-test/deslag" \
  "$(tar -tzf "$r/dist/deslag-1.0.0-x-test.tar.gz" | sort | tr '\n' ' ' | sed 's/ $//')"
expect_fail "package fails without a built binary" "not found" "$r" "$here/package.sh" 1.0.0 missing-target
expect_ok "checksums writes SHA256SUMS" "$r" "$here/checksums.sh" dist
expect_eq "SHA256SUMS lists the archive" "deslag-1.0.0-x-test.tar.gz" "$(awk '{print $2}' "$r/dist/SHA256SUMS")"
if command -v sha256sum >/dev/null; then verify=(sha256sum -c); else verify=(shasum -a 256 -c); fi
expect_ok "SHA256SUMS verifies" "$r/dist" "${verify[@]}" SHA256SUMS

# --- release-state.sh --------------------------------------------------------
# A fake `gh release view`: FAKE_VIEW_ERR set -> exit 1 with that stderr, else
# print FAKE_VIEW_ASSETS (one name per line).
mkdir -p "$tmp/viewbin"
cat >"$tmp/viewbin/gh" <<'EOF'
#!/usr/bin/env bash
[ -z "${FAKE_VIEW_ERR:-}" ] || { echo "$FAKE_VIEW_ERR" >&2; exit 1; }
printf '%s' "${FAKE_VIEW_ASSETS:-}"
EOF
chmod +x "$tmp/viewbin/gh"
state() { PATH="$tmp/viewbin:$PATH" "$here/release-state.sh" v1.0.0 dist/a.tar.gz dist/b.tar.gz dist/SHA256SUMS; }
expect_eq "release-state creates when the release is not found" create "$(FAKE_VIEW_ERR='release not found' state 2>/dev/null)"
FAKE_VIEW_ERR='HTTP 502: Bad Gateway (https://api.github.com/graphql)' expect_fail "release-state fails on an HTTP 5xx" "could not look up" "$tmp" state
FAKE_VIEW_ERR='HTTP 401: Bad credentials (https://api.github.com/graphql)' expect_fail "release-state fails on HTTP 401" "could not look up" "$tmp" state
FAKE_VIEW_ERR='HTTP 403: Resource not accessible by integration' expect_fail "release-state fails on HTTP 403" "could not look up" "$tmp" state
FAKE_VIEW_ERR='error connecting to api.github.com' expect_fail "release-state fails on any other gh error" "could not look up" "$tmp" state
expect_eq "release-state skips a complete release" skip "$(FAKE_VIEW_ASSETS=$'a.tar.gz\nb.tar.gz\nSHA256SUMS\n' state 2>/dev/null)"
expect_eq "release-state uploads into an empty release" upload "$(FAKE_VIEW_ASSETS='' state 2>/dev/null)"
FAKE_VIEW_ASSETS=$'a.tar.gz\n' expect_fail "release-state fails on a partial release" "partial \(missing: b.tar.gz SHA256SUMS\)" "$tmp" state

# --- publish.sh --------------------------------------------------------------
# A stateful fake `gh release`: release <tag> is the directory
# $FAKE_RELEASES/<tag>, and its assets are the files in it. create and upload
# refuse an existing asset name, like gh without --clobber, and check every
# name before they copy any file. Every call is appended to $FAKE_GH_LOG.
mkdir -p "$tmp/pubbin"
cat >"$tmp/pubbin/gh" <<'EOF'
#!/usr/bin/env bash
echo "$*" >>"$FAKE_GH_LOG"
sub=$2 rel=$FAKE_RELEASES/$3; shift 3
assets=()
for a; do case $a in -*) break ;; *) assets+=("$a") ;; esac; done
case $sub in
  view)
    [ -z "${FAKE_VIEW_ERR:-}" ] || { echo "$FAKE_VIEW_ERR" >&2; exit 1; }
    [ -d "$rel" ] || { echo "release not found" >&2; exit 1; }
    ls "$rel" ;;
  create|upload)
    [ -z "${FAKE_WRITE_ERR:-}" ] || { echo "$FAKE_WRITE_ERR" >&2; exit 1; }
    if [ "$sub" = create ] && [ -d "$rel" ]; then echo "release already exists" >&2; exit 1; fi
    if [ "$sub" = upload ] && [ ! -d "$rel" ]; then echo "release not found" >&2; exit 1; fi
    for a in "${assets[@]}"; do
      [ ! -e "$rel/${a##*/}" ] || { echo "asset under the same name already exists: ${a##*/}" >&2; exit 1; }
    done
    mkdir -p "$rel" && cp "${assets[@]}" "$rel/" ;;
  *) echo "fake gh: unexpected release $sub" >&2; exit 2 ;;
esac
EOF
chmod +x "$tmp/pubbin/gh"
r=$tmp/publish; mkdir -p "$r/dist"
for f in a.tar.gz b.tar.gz SHA256SUMS; do echo "built $f" >"$r/dist/$f"; done
echo "Notes." >"$r/release-notes.md"
export FAKE_RELEASES=$tmp/releases FAKE_GH_LOG=$tmp/gh.log
mkdir -p "$FAKE_RELEASES"
# run_release <version>: the publish job, release-state.sh then publish.sh.
run_release() {
  local action
  action=$(PATH="$tmp/pubbin:$PATH" "$here/release-state.sh" "v$1" dist/*.tar.gz dist/SHA256SUMS)
  PATH="$tmp/pubbin:$PATH" "$here/publish.sh" "$1" "$action"
}
assets_of() { (cd "$FAKE_RELEASES/$1" && echo *); }
snapshot() { (cd "$FAKE_RELEASES" && find . -type f -exec cksum {} + | sort); }
writes() { grep -cE '^release (create|upload)' "$FAKE_GH_LOG" || true; }

: >"$FAKE_GH_LOG"
expect_ok "publish creates a missing release" "$r" run_release 1.0.0-rc.1
expect_eq "publish creates the release with every asset" "SHA256SUMS a.tar.gz b.tar.gz" "$(assets_of v1.0.0-rc.1)"
expect_eq "publish creates a prerelease with the notes" \
  "release create v1.0.0-rc.1 dist/a.tar.gz dist/b.tar.gz dist/SHA256SUMS --verify-tag --title v1.0.0-rc.1 --notes-file release-notes.md --prerelease" \
  "$(grep '^release create' "$FAKE_GH_LOG")"
: >"$FAKE_GH_LOG"
expect_ok "publish creates a stable release" "$r" run_release 1.0.0
expect_eq "publish does not mark a stable release as a prerelease" \
  "release create v1.0.0 dist/a.tar.gz dist/b.tar.gz dist/SHA256SUMS --verify-tag --title v1.0.0 --notes-file release-notes.md" \
  "$(grep '^release create' "$FAKE_GH_LOG")"

: >"$FAKE_GH_LOG"; before=$(snapshot)
expect_ok "publish re-run on a complete release succeeds" "$r" run_release 1.0.0
expect_eq "publish re-run on a complete release calls no write" 0 "$(writes)"
expect_eq "publish re-run on a complete release changes nothing" "$before" "$(snapshot)"

mkdir "$FAKE_RELEASES/v2.0.0"; : >"$FAKE_GH_LOG"
expect_ok "publish re-run on an empty release succeeds" "$r" run_release 2.0.0
expect_eq "publish re-run on an empty release uploads every asset" "SHA256SUMS a.tar.gz b.tar.gz" "$(assets_of v2.0.0)"
expect_eq "publish re-run on an empty release only uploads" \
  "release upload v2.0.0 dist/a.tar.gz dist/b.tar.gz dist/SHA256SUMS" "$(grep -E '^release (create|upload)' "$FAKE_GH_LOG")"

mkdir "$FAKE_RELEASES/v3.0.0"; echo "published a.tar.gz" >"$FAKE_RELEASES/v3.0.0/a.tar.gz"
: >"$FAKE_GH_LOG"; before=$(snapshot)
expect_fail "publish re-run on a partial release fails" "partial \(missing: b.tar.gz SHA256SUMS\)" "$r" run_release 3.0.0
expect_eq "publish re-run on a partial release calls no write" 0 "$(writes)"
expect_eq "publish re-run on a partial release keeps the published asset" "$before" "$(snapshot)"
: >"$FAKE_GH_LOG"
expect_fail "publish upload never replaces an asset" "already exists: a.tar.gz" "$r" env PATH="$tmp/pubbin:$PATH" "$here/publish.sh" 3.0.0 upload
expect_eq "publish upload keeps the published asset" "$before" "$(snapshot)"

: >"$FAKE_GH_LOG"; before=$(snapshot)
FAKE_VIEW_ERR='HTTP 502: Bad Gateway' expect_fail "publish fails on an unexpected gh lookup error" "could not look up" "$r" run_release 4.0.0
expect_eq "publish calls no write after a gh lookup error" 0 "$(writes)"
expect_eq "publish creates no release after a gh lookup error" "$before" "$(snapshot)"
FAKE_WRITE_ERR='HTTP 500: Internal Server Error' expect_fail "publish fails when gh release create fails" "HTTP 500" "$r" run_release 4.0.0
expect_eq "publish leaves no release after a failed create" "$before" "$(snapshot)"
: >"$FAKE_GH_LOG"
expect_fail "publish rejects an unknown action" "unknown action" "$r" env PATH="$tmp/pubbin:$PATH" "$here/publish.sh" 4.0.0 replace
expect_eq "publish calls no gh on an unknown action" "" "$(cat "$FAKE_GH_LOG")"
expect_fail "publish fails without archives" "no archives" "$tmp" env PATH="$tmp/pubbin:$PATH" "$here/publish.sh" 4.0.0 create
expect_eq "publish calls no gh without archives" "" "$(cat "$FAKE_GH_LOG")"

if [ "$failures" -ne 0 ]; then echo "$failures test(s) failed" >&2; exit 1; fi
echo "all release script tests passed"
