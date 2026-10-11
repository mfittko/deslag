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
  mkdir -p "$dir/crates/a/src" "$dir/plugins/claude/.claude-plugin" "$dir/changes"
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
name = "a"
version.workspace = true
edition.workspace = true
EOF
  : >"$dir/crates/a/src/lib.rs"
  printf '{\n  "name": "deslag",\n  "version": "0.1.0"\n}\n' >"$dir/plugins/claude/.claude-plugin/plugin.json"
  printf '# Changelog\n\n## Unreleased\n\n## 0.0.1 - 2020-01-01\n\n- First.\n' >"$dir/CHANGELOG.md"
  echo "# Fragments" >"$dir/changes/README.md"
  echo "# readme" >"$dir/README.md"
  (cd "$dir" && git init -q -b main && cargo generate-lockfile --offline -q && git add -A && git commit -qm base)
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
set_version() {
  sed "s/^version = .*/version = \"$1\"/" "$r/Cargo.toml" >"$r/Cargo.toml.new" && command mv -f "$r/Cargo.toml.new" "$r/Cargo.toml"
  printf '{\n  "version": "%s"\n}\n' "$1" >"$r/plugins/claude/.claude-plugin/plugin.json"
}
for v in 0.1.0-alpha.1 1.0.0-rc.1 1.0.0; do
  set_version "$v"
  expect_ok "check-versions accepts $v" "$r" "$here/check-versions.sh" "v$v"
done
for v in 1.0.0+build 1.0.0+build-1; do
  set_version "$v"
  expect_fail "check-versions refuses build metadata ($v)" "not a semver" "$r" "$here/check-versions.sh" "v$v"
done

# --- check-changelog-fragment.sh --------------------------------------------
r=$(new_repo fragments)
base=$(git -C "$r" rev-parse HEAD)
branch() { git -C "$r" checkout -q -B "$1" "$base"; }
commit() { git -C "$r" add -A && git -C "$r" commit -qm "$1"; }
check() { PR_BODY=$1 "$here/check-changelog-fragment.sh" "$base"; }

branch docs-only; echo more >>"$r/README.md"; commit docs
expect_ok "fragment check passes a change outside crates/ and plugins/" "$r" check ""

branch no-fragment; echo "// x" >>"$r/crates/a/src/lib.rs"; commit code
expect_fail "fragment check fails crates/ change without fragment" "changes/" "$r" check ""
expect_ok "fragment check accepts 'changelog: none'" "$r" check "Summary"$'\n'"  Changelog: None  "$'\r'
expect_fail "fragment check rejects 'changelog: none' inside a sentence" "changes/" "$r" check "no changelog: none needed"

branch plugin; echo " " >>"$r/plugins/claude/.claude-plugin/plugin.json"; commit plugin
expect_fail "fragment check fails plugins/ change without fragment" "changes/" "$r" check ""

branch with-fragment; echo "// y" >>"$r/crates/a/src/lib.rs"; printf -- '- Thing.\n' >"$r/changes/thing.md"; commit frag
expect_ok "fragment check accepts a non-empty fragment" "$r" check ""

branch empty-fragment; echo "// z" >>"$r/crates/a/src/lib.rs"; printf '\n  \n' >"$r/changes/empty.md"; commit empty
expect_fail "fragment check rejects an empty fragment" "changes/" "$r" check ""

branch readme-only; echo "// w" >>"$r/crates/a/src/lib.rs"; echo more >>"$r/changes/README.md"; commit readme
expect_fail "fragment check ignores changes/README.md" "changes/" "$r" check ""

branch bad-fragment; echo "// v" >>"$r/crates/a/src/lib.rs"; printf 'Just text.\n' >"$r/changes/bad.md"; commit bad
expect_fail "fragment check rejects a line that is no entry" "expected a" "$r" check ""

branch bad-heading; echo "// u" >>"$r/crates/a/src/lib.rs"; printf -- '### Removed\n- Gone.\n' >"$r/changes/bad.md"; commit bad
expect_fail "fragment check rejects an unknown section heading" "unknown section" "$r" check ""

branch one-bad; echo "// t" >>"$r/crates/a/src/lib.rs"; printf -- '- Fine.\n' >"$r/changes/good.md"; printf '## Heading\n- X.\n' >"$r/changes/bad.md"; commit bad
expect_fail "fragment check rejects a malformed fragment next to a valid one" "expected a" "$r" check ""

branch crlf-fragment; echo "// s" >>"$r/crates/a/src/lib.rs"; printf -- '### Fixed\r\n- Fine.\r\n' >"$r/changes/crlf.md"; commit crlf
expect_ok "fragment check accepts a CRLF fragment" "$r" check ""

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
  "CHANGELOG.md Cargo.lock Cargo.toml changes/feature.md changes/tweak.md changes/zfix.md plugins/claude/.claude-plugin/plugin.json" \
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

r=$(new_repo bump-cargo-fail)
printf -- '- Change.\n' >"$r/changes/change.md"
git -C "$r" add -A && git -C "$r" commit -qm pending
mkdir -p "$tmp/fakecargo"
printf '#!/bin/sh\necho "cargo: simulated failure" >&2\nexit 101\n' >"$tmp/fakecargo/cargo"
chmod +x "$tmp/fakecargo/cargo"
expect_fail "bump fails when cargo update fails" "simulated failure" "$r" env PATH="$tmp/fakecargo:$PATH" "$here/bump.sh" 0.2.0
expect_eq "bump restores every file when cargo update fails" "" "$(git -C "$r" status --porcelain)"

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
cat "$FAKE_GH_JSON"
EOF
chmod +x "$tmp/bin/gh"
export FAKE_GH_JSON=$tmp/comments.json
since=2026-01-01T00:00:00Z
comments() { printf '%s\n' "$@" >"$FAKE_GH_JSON"; }
c() { printf '{"user":{"login":"%s"},"created_at":"%s","body":"%s","html_url":"u"}' "$1" "$2" "$3"; }
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
comments "[$(c mfittko 2025-12-31T23:59:59Z 'approve release v1.0.0')]"
expect_fail "approval rejects a comment older than the release commit" "no approval" "$r" approve 1.0.0
comments "[$(c mfittko 2026-01-02T00:00:00Z 'I do not approve release v1.0.0')]"
expect_fail "approval rejects the phrase inside a sentence" "no approval" "$r" approve 1.0.0
comments "[$(c mfittko 2026-01-02T00:00:00Z 'approve release v1.0.1')]"
expect_fail "approval rejects another version" "no approval" "$r" approve 1.0.0
comments "[$(c x 2026-01-02T00:00:00Z a)]" "[$(c mfittko 2026-01-03T00:00:00Z 'approve release v1.0.0')]"
expect_ok "approval reads every paginated page" "$r" approve 1.0.0
FAKE_GH_FAIL=1 expect_fail "approval fails closed when gh fails" "could not list" "$r" approve 1.0.0
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

if [ "$failures" -ne 0 ]; then echo "$failures test(s) failed" >&2; exit 1; fi
echo "all release script tests passed"
