#!/usr/bin/env bash
# Self-contained tests for the plugin install script and hook wrapper.
# Release archives are built in a temp dir and fetched over file:// URLs, so
# no test touches the network.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
install_sh=$root/scripts/install.sh
hook_sh=$root/hooks/deslag-hook.sh
tmp=$(mktemp -d)
trap 'command rm -rf "$tmp"' EXIT
export LC_ALL=C
unset DESLAG_RELEASE_BASE_URL
failures=0

ok() { echo "ok - $1"; }
not_ok() { echo "not ok - $1"; printf '    %s\n' "${2//$'\n'/$'\n'    }"; failures=$((failures + 1)); }
expect_eq() {
  if [ "$2" = "$3" ]; then ok "$1"; else not_ok "$1" "$(printf 'expected:\n%s\nactual:\n%s' "$2" "$3")"; fi
}
# expect_ok <desc> <cmd...>
expect_ok() {
  local desc=$1 out; shift
  if out=$("$@" 2>&1); then ok "$desc"; else not_ok "$desc" "$out"; fi
}
# expect_fail <desc> <pattern> <cmd...>
expect_fail() {
  local desc=$1 pattern=$2 out; shift 2
  if out=$("$@" 2>&1); then not_ok "$desc (unexpected success)" "$out"
  elif grep -qiE -- "$pattern" <<<"$out"; then ok "$desc"
  else not_ok "$desc (output did not match /$pattern/)" "$out"; fi
}
# expect_match <desc> <pattern> <text>
expect_match() {
  if grep -qiE -- "$2" <<<"$3"; then ok "$1"; else not_ok "$1 (no match for /$2/)" "$3"; fi
}
mode() { if stat -f %Lp "$1" >/dev/null 2>&1; then stat -f %Lp "$1"; else stat -c %a "$1"; fi; }
sha256() { if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi; }

version=$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$root/.claude-plugin/plugin.json")
targets="aarch64-apple-darwin x86_64-unknown-linux-gnu"

# make_release <dir> <payload>: one archive per target with a fake `deslag`
# that prints <payload>, its args and its stdin, plus SHA256SUMS.
make_release() {
  local dir=$1/v$version stage t
  mkdir -p "$dir"
  for t in $targets; do
    stage=$(mktemp -d "$tmp/stage.XXXXXX")
    mkdir "$stage/deslag-$version-$t"
    printf '#!/bin/sh\necho "%s $*"\ncat\n' "$2" >"$stage/deslag-$version-$t/deslag"
    chmod +x "$stage/deslag-$version-$t/deslag"
    COPYFILE_DISABLE=1 tar -C "$stage" -czf "$dir/deslag-$version-$t.tar.gz" "deslag-$version-$t"
  done
  (cd "$dir" && sha256 ./*.tar.gz | sed 's| \./| |' >SHA256SUMS)
}

good=$tmp/good && make_release "$good" fake-deslag
# The plugin commands pass the data dir as an argument and export no plugin env.
run_install() { # run_install <base> <data> <subcommand>
  DESLAG_RELEASE_BASE_URL=file://$1 env -u CLAUDE_PLUGIN_ROOT -u CLAUDE_PLUGIN_DATA "$install_sh" "$3" "$2"
}
run_install_env() { # run_install_env <base> <data> <subcommand>: data from CLAUDE_PLUGIN_DATA
  DESLAG_RELEASE_BASE_URL=file://$1 CLAUDE_PLUGIN_DATA=$2 env -u CLAUDE_PLUGIN_ROOT "$install_sh" "$3"
}
run_hook() { # run_hook <data>; stdin passes through
  env -i PATH="$tmp/fakebin:/usr/bin:/bin" CLAUDE_PLUGIN_ROOT="$root" CLAUDE_PLUGIN_DATA="$1" "$hook_sh"
}

# --- install -----------------------------------------------------------------
data=$tmp/data
mkdir -p "$data/bin/0.0.1" && echo old >"$data/bin/0.0.1/deslag"
expect_ok "install succeeds with a matching archive" run_install "$good" "$data" install
expect_eq "install puts the binary under bin/<version>" "fake-deslag --version" "$("$data/bin/$version/deslag" --version </dev/null)"
expect_eq "data dir is private" 700 "$(mode "$data")"
expect_eq "bin dir is private" 700 "$(mode "$data/bin")"
expect_eq "install prunes other versions" "$version" "$(ls "$data/bin")"
expect_eq "install leaves no staging dir" "bin" "$(ls "$data")"
expect_ok "install is repeatable" run_install "$good" "$data" install

tampered=$tmp/tampered && make_release "$tampered" fake-deslag
make_release "$tmp/evil" evil-deslag
for t in $targets; do
  command cp -f "$tmp/evil/v$version/deslag-$version-$t.tar.gz" "$tampered/v$version/"
done
expect_fail "install rejects a tampered archive" "checksum mismatch" run_install "$tampered" "$tmp/data-tampered" install
expect_eq "tampered archive installs nothing" "" "$(ls "$tmp/data-tampered/bin" 2>/dev/null || true)"

noline=$tmp/noline && make_release "$noline" fake-deslag
: >"$noline/v$version/SHA256SUMS"
expect_fail "install rejects an archive missing from SHA256SUMS" "no checksum" run_install "$noline" "$tmp/data-noline" install
expect_eq "missing checksum installs nothing" "" "$(ls "$tmp/data-noline/bin" 2>/dev/null || true)"

expect_fail "install fails when the release is missing" "download" run_install "$tmp/nowhere" "$tmp/data-404" install
expect_fail "install requires a data dir" "CLAUDE_PLUGIN_DATA" run_install "$good" "" install
expect_fail "install refuses a relative data dir" "absolute" run_install "$good" "rel/data" install
expect_fail "install refuses a relative CLAUDE_PLUGIN_DATA" "absolute" run_install_env "$good" "rel/data" install
expect_ok "install falls back to CLAUDE_PLUGIN_DATA" run_install_env "$good" "$tmp/data-env" install
expect_eq "env fallback installs under bin/<version>" "fake-deslag --version" "$("$tmp/data-env/bin/$version/deslag" --version </dev/null)"
expect_fail "install rejects an unknown subcommand" "usage" run_install "$good" "$data" frobnicate

# A user-managed binary on PATH is never touched.
mkdir -p "$tmp/userbin" && printf '#!/bin/sh\necho user\n' >"$tmp/userbin/deslag" && chmod 755 "$tmp/userbin/deslag"
before=$(sha256 "$tmp/userbin/deslag")
PATH="$tmp/userbin:$PATH" run_install "$good" "$tmp/data-user" install >/dev/null 2>&1
expect_eq "install leaves a user-managed binary unchanged" "$before" "$(sha256 "$tmp/userbin/deslag")"

# --- status ------------------------------------------------------------------
expect_match "status reports an installed binary" "installed" "$(run_install "$good" "$data" status)"
out=$(run_install "$good" "$tmp/data-none" status)
expect_match "status reports a missing binary and the fix" "missing.*/deslag:install" "$(tr '\n' ' ' <<<"$out")"
mkdir -p "$tmp/data-old/bin/0.0.1"
expect_match "status reports a version mismatch" "mismatch.*/deslag:install" "$(run_install "$good" "$tmp/data-old" status | tr '\n' ' ')"

# --- hook ----------------------------------------------------------------------
# Fake network tools record any call.
mkdir -p "$tmp/fakebin"
for tool in curl wget; do
  printf '#!/bin/sh\necho %s >>"%s/network.log"\n' "$tool" "$tmp" >"$tmp/fakebin/$tool"
  chmod +x "$tmp/fakebin/$tool"
done
rc=0; out=$(echo '{"tool_name":"Bash"}' | run_hook "$tmp/data-none") || rc=$?
expect_eq "hook with a missing binary exits 0" 0 "$rc"
expect_eq "hook with a missing binary prints nothing" "" "$out"
rc=0; out=$(echo '{"tool_name":"Bash"}' | run_hook "$tmp/data-old") || rc=$?
expect_eq "hook with a mismatched version exits 0" 0 "$rc"
expect_eq "hook with a mismatched version prints nothing" "" "$out"
rc=0; out=$(echo '{"tool_name":"Bash"}' | env -i PATH=/usr/bin:/bin "$hook_sh" 2>&1) || rc=$?
expect_eq "hook without plugin env exits 0 silently" "0:" "$rc:$out"
expect_eq "hook runs the installed binary with stdin" \
  "$(printf 'fake-deslag hook claude\n{"tool_name":"Bash"}')" \
  "$(echo '{"tool_name":"Bash"}' | run_hook "$data")"
expect_eq "hook makes no network call" "" "$(cat "$tmp/network.log" 2>/dev/null || true)"

hooks_json=$root/hooks/hooks.json
expect_eq "hooks.json registers only PostToolUse" '["PostToolUse"]' "$(jq -c '.hooks | keys' "$hooks_json")"
# shellcheck disable=SC2016 # the literal placeholder is expected
expect_eq "hooks.json runs only the wrapper" '["\"${CLAUDE_PLUGIN_ROOT}/hooks/deslag-hook.sh\""]' \
  "$(jq -c '[.hooks[][].hooks[].command] | unique' "$hooks_json")"

# --- commands ------------------------------------------------------------------
# The client substitutes ${CLAUDE_PLUGIN_DATA} in command text but does not
# export it, so each command must pass it to install.sh as an argument.
for c in install status uninstall; do
  # shellcheck disable=SC2016 # the literal placeholder is expected
  expect_eq "commands/$c.md passes the data dir as an argument" \
    '!`${CLAUDE_PLUGIN_ROOT}/scripts/install.sh '"$c"' "${CLAUDE_PLUGIN_DATA}"`' \
    "$(grep '^!`' "$root/commands/$c.md")"
done

# --- uninstall -----------------------------------------------------------------
expect_ok "uninstall succeeds" run_install "$good" "$data" uninstall
expect_eq "uninstall removes the data dir" "absent" "$([ -e "$data" ] && echo present || echo absent)"
mkdir -p "$tmp/foreign" && echo keep >"$tmp/foreign/file"
expect_fail "uninstall refuses a dir without the marker" "refus" run_install "$good" "$tmp/foreign" uninstall
expect_eq "refused uninstall keeps the dir" keep "$(cat "$tmp/foreign/file")"
expect_ok "uninstall of a missing data dir is a no-op" run_install "$good" "$tmp/data-gone" uninstall

if [ "$failures" -ne 0 ]; then echo "$failures test(s) failed" >&2; exit 1; fi
echo "all plugin tests passed"
