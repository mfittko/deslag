#!/usr/bin/env bash
# Self-contained tests for the plugin install script and hook wrapper.
# Release archives are built in a temp dir and fetched over file:// URLs, so
# no test touches the network.
set -euo pipefail

root=$(cd "$(dirname "$0")/../../plugins/claude" && pwd)
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

# make_release <dir> <payload> [renamed|symlink]: one archive per target with a
# fake `deslag` that prints <payload>, its args and its stdin, plus SHA256SUMS.
# `renamed` packs a wrong top directory; `symlink` packs deslag as a symlink.
make_release() {
  local dir=$1/v$version stage t top
  mkdir -p "$dir"
  for t in $targets; do
    stage=$(mktemp -d "$tmp/stage.XXXXXX")
    top=deslag-$version-$t
    [ "${3:-}" != renamed ] || top=other-$t
    mkdir "$stage/$top"
    printf '#!/bin/sh\necho "%s $*"\ncat\n' "$2" >"$stage/$top/deslag"
    chmod +x "$stage/$top/deslag"
    if [ "${3:-}" = symlink ]; then
      command mv "$stage/$top/deslag" "$stage/$top/real" && ln -s real "$stage/$top/deslag"
    fi
    COPYFILE_DISABLE=1 tar -C "$stage" -czf "$dir/deslag-$version-$t.tar.gz" "$top"
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
expect_eq "install leaves no staging dir" "" "$(find "$data" -maxdepth 1 -name ".stage.*")"
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

make_release "$tmp/renamed" fake-deslag renamed
expect_fail "install rejects an archive with a wrong top directory" "archive has no" run_install "$tmp/renamed" "$tmp/data-renamed" install
expect_eq "wrong top directory installs nothing" "" "$(ls "$tmp/data-renamed/bin" 2>/dev/null || true)"
make_release "$tmp/symmember" fake-deslag symlink
expect_fail "install rejects a symlinked deslag member" "regular file" run_install "$tmp/symmember" "$tmp/data-symmember" install
expect_eq "symlinked deslag member installs nothing" "" "$(ls "$tmp/data-symmember/bin" 2>/dev/null || true)"

expect_fail "install fails when the release is missing" "download" run_install "$tmp/nowhere" "$tmp/data-404" install
expect_fail "install requires a data dir" "CLAUDE_PLUGIN_DATA" run_install "$good" "" install
expect_fail "install refuses a relative data dir" "absolute" run_install "$good" "rel/data" install
expect_fail "install refuses a relative CLAUDE_PLUGIN_DATA" "absolute" run_install_env "$good" "rel/data" install
expect_ok "install falls back to CLAUDE_PLUGIN_DATA" run_install_env "$good" "$tmp/data-env" install
expect_eq "env fallback installs under bin/<version>" "fake-deslag --version" "$("$tmp/data-env/bin/$version/deslag" --version </dev/null)"
expect_fail "install rejects an unknown subcommand" "usage" run_install "$good" "$data" frobnicate
expect_fail "install.sh without a subcommand prints usage" "usage" env -u CLAUDE_PLUGIN_DATA "$install_sh"

# A data path with a space and a quote must not break the cleanup trap.
odd="$tmp/odd dir's data"
expect_ok "install accepts a data path with a space and a quote" run_install "$good" "$odd" install
expect_eq "odd data path leaves no staging dir" "" "$(find "$odd" -maxdepth 1 -name ".stage.*")"

# A symlinked old version dir is removed as a link; its target survives.
mkdir -p "$tmp/linked-old" && echo keep >"$tmp/linked-old/deslag"
mkdir -p "$tmp/data-link/bin" && ln -s "$tmp/linked-old" "$tmp/data-link/bin/0.0.1"
expect_ok "install with a symlinked old version dir succeeds" run_install "$good" "$tmp/data-link" install
expect_eq "install removes the old version link" "absent" "$([ -L "$tmp/data-link/bin/0.0.1" ] && echo present || echo absent)"
expect_eq "install keeps the link target" keep "$(cat "$tmp/linked-old/deslag")"

# A symlinked current version dir is user-managed; install never writes through it.
mkdir -p "$tmp/linked-cur" && echo keep >"$tmp/linked-cur/deslag"
mkdir -p "$tmp/data-curlink/bin" && ln -s "$tmp/linked-cur" "$tmp/data-curlink/bin/$version"
expect_fail "install refuses a symlinked current version dir" "symlink" run_install "$good" "$tmp/data-curlink" install
expect_eq "refused install leaves the link target unchanged" "deslag:keep" "$(ls -A "$tmp/linked-cur"):$(cat "$tmp/linked-cur/deslag")"

# A symlinked data dir would let every install step act on its target.
mkdir -p "$tmp/linked-data" && echo keep >"$tmp/linked-data/notes.txt" && chmod 755 "$tmp/linked-data"
ln -s "$tmp/linked-data" "$tmp/data-datalink"
expect_fail "install refuses a symlinked data dir" "symlink" run_install "$good" "$tmp/data-datalink" install
expect_fail "install refuses a symlinked data dir with a trailing slash" "symlink" run_install "$good" "$tmp/data-datalink/" install
expect_fail "install refuses a data dir with a . component" "components" run_install "$good" "$tmp/data-datalink/." install
expect_fail "install refuses a data dir with a .. component" "components" run_install "$good" "$tmp/data-datalink/sub/.." install
expect_eq "refused install leaves the data link target unchanged" "notes.txt:keep:755" \
  "$(ls -A "$tmp/linked-data"):$(cat "$tmp/linked-data/notes.txt"):$(mode "$tmp/linked-data")"

# A symlinked bin dir would let chmod and the prune loop act on its target.
mkdir -p "$tmp/linked-bin" && echo keep >"$tmp/linked-bin/notes.txt" && chmod 755 "$tmp/linked-bin"
mkdir -p "$tmp/data-binlink" && ln -s "$tmp/linked-bin" "$tmp/data-binlink/bin"
expect_fail "install refuses a symlinked bin dir" "symlink" run_install "$good" "$tmp/data-binlink" install
expect_eq "refused install leaves the bin link target unchanged" "notes.txt:keep:755" \
  "$(ls -A "$tmp/linked-bin"):$(cat "$tmp/linked-bin/notes.txt"):$(mode "$tmp/linked-bin")"

# A symlinked binary path would let mv write into its target.
mkdir -p "$tmp/linked-bindir" && echo keep >"$tmp/linked-bindir/deslag"
mkdir -p "$tmp/data-dirlink/bin/$version" && ln -s "$tmp/linked-bindir" "$tmp/data-dirlink/bin/$version/deslag"
expect_fail "install refuses a binary path linked to a directory" "refusing to install" run_install "$good" "$tmp/data-dirlink" install
expect_eq "refused install leaves the linked directory unchanged" "deslag:keep" "$(ls -A "$tmp/linked-bindir"):$(cat "$tmp/linked-bindir/deslag")"
echo keep >"$tmp/linked-binfile"
mkdir -p "$tmp/data-filelink/bin/$version" && ln -s "$tmp/linked-binfile" "$tmp/data-filelink/bin/$version/deslag"
expect_fail "install refuses a binary path linked to a file" "refusing to install" run_install "$good" "$tmp/data-filelink" install
expect_eq "refused install leaves the linked file unchanged" keep "$(cat "$tmp/linked-binfile")"
# A regular file at the binary path is replaced.
mkdir -p "$tmp/data-regular/bin/$version" && echo stale >"$tmp/data-regular/bin/$version/deslag"
expect_ok "install replaces a regular file at the binary path" run_install "$good" "$tmp/data-regular" install
expect_eq "replaced file is the release binary" "fake-deslag --version" "$("$tmp/data-regular/bin/$version/deslag" --version </dev/null)"

# A symlinked marker would let the marker write truncate its target.
echo keep >"$tmp/linked-marker"
mkdir -p "$tmp/data-markerlink" && ln -s "$tmp/linked-marker" "$tmp/data-markerlink/.deslag-data"
expect_fail "install refuses a symlinked marker" "symlink" run_install "$good" "$tmp/data-markerlink" install
expect_eq "refused install leaves the marker link target unchanged" keep "$(cat "$tmp/linked-marker")"
expect_eq "refused marker install creates no bin dir" absent "$([ -e "$tmp/data-markerlink/bin" ] && echo present || echo absent)"

# A user-managed binary on PATH is never touched.
mkdir -p "$tmp/userbin" && printf '#!/bin/sh\necho user\n' >"$tmp/userbin/deslag" && chmod 755 "$tmp/userbin/deslag"
before=$(sha256 "$tmp/userbin/deslag")
PATH="$tmp/userbin:$PATH" run_install "$good" "$tmp/data-user" install >/dev/null 2>&1
expect_eq "install leaves a user-managed binary unchanged" "$before" "$(sha256 "$tmp/userbin/deslag")"

# --- status ------------------------------------------------------------------
expect_match "status reports an installed binary" "installed" "$(run_install "$good" "$data" status)"
out=$(run_install "$good" "$tmp/data-none" status)
expect_match "status reports a missing binary and the fix" "missing.*/deslag:install" "$(tr '\n' ' ' <<<"$out")"
# data-old holds a working binary for another version only.
[ "$version" != 0.0.1 ] || { echo "fixture version 0.0.1 collides with the plugin version" >&2; exit 1; }
mkdir -p "$tmp/data-old/bin/0.0.1"
printf '#!/bin/sh\necho old-deslag "$@"\n' >"$tmp/data-old/bin/0.0.1/deslag" && chmod 755 "$tmp/data-old/bin/0.0.1/deslag"
expect_match "status reports a version mismatch" "mismatch.*/deslag:install" "$(run_install "$good" "$tmp/data-old" status | tr '\n' ' ')"
mkdir -p "$tmp/data-empty/bin/$version" "$tmp/data-empty/bin/0.0.1"
expect_match "status reports empty version dirs as missing" "missing.*/deslag:install" "$(run_install "$good" "$tmp/data-empty" status | tr '\n' ' ')"
mkdir -p "$tmp/data-partial/bin/$version" && echo partial >"$tmp/data-partial/bin/$version/deslag"
expect_match "status reports a non-executable binary as missing" "missing.*/deslag:install" "$(run_install "$good" "$tmp/data-partial" status | tr '\n' ' ')"

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
for d in data-partial data-empty; do
  rc=0; out=$(echo '{"tool_name":"Bash"}' | run_hook "$tmp/$d" 2>&1) || rc=$?
  expect_eq "hook with $d exits 0 with no output" "0:" "$rc:$out"
done
# Without a binary the wrapper must read all input, so a large write never hits a broken pipe.
for d in data-gone data-old; do
  rc=0; out=$( { head -c 1048576 /dev/zero; echo "writer=$?" >"$tmp/writer-$d"; } | run_hook "$tmp/$d" 2>&1) || rc=$?
  expect_eq "hook with $d drains 1 MiB of input and exits 0 silently" "0::writer=0" "$rc:$out:$(cat "$tmp/writer-$d")"
done
rc=0; out=$(echo '{"tool_name":"Bash"}' | env -i PATH=/usr/bin:/bin "$hook_sh" 2>&1) || rc=$?
expect_eq "hook without plugin env exits 0 silently" "0:" "$rc:$out"
expect_eq "hook runs the installed binary with stdin" \
  "$(printf 'fake-deslag hook claude\n{"tool_name":"Bash"}')" \
  "$(echo '{"tool_name":"Bash"}' | run_hook "$data")"
# install.sh and the hook wrapper parse the same version from the real plugin.json.
run_install "$good" "$tmp/data-shared" install >/dev/null
expect_eq "hook runs the binary that install.sh installed" "fake-deslag hook claude" \
  "$(run_hook "$tmp/data-shared" </dev/null)"
expect_eq "hook makes no network call" "" "$(cat "$tmp/network.log" 2>/dev/null || true)"

# --- compression -----------------------------------------------------------------
compression_sh=$root/scripts/compression.sh
run_compression() { # run_compression <env...> -- <args...>: only the given env is set
  local vars=()
  while [ "$1" != -- ]; do vars+=("$1"); shift; done
  shift
  env -i PATH=/usr/bin:/bin "${vars[@]}" "$compression_sh" "$@"
}
run_hook_env() { # run_hook_env <data> <env...>; stdin passes through
  local d=$1; shift
  env -i PATH="$tmp/fakebin:/usr/bin:/bin" CLAUDE_PLUGIN_ROOT="$root" CLAUDE_PLUGIN_DATA="$d" "$@" "$hook_sh"
}
st=$tmp/state
out=$(run_compression DESLAG_STATE_DIR="$st" -- status)
expect_match "compression status defaults to off" "compression: off" "$out"
expect_match "compression status reports a missing owner as none" "owner: none" "$out"
expect_eq "compression status creates nothing" absent "$([ -e "$st" ] && echo present || echo absent)"
expect_eq "hook without a compression file runs without --minify-json" "fake-deslag hook claude" \
  "$(run_hook_env "$data" DESLAG_STATE_DIR="$st" </dev/null)"

expect_ok "compression on succeeds" run_compression DESLAG_STATE_DIR="$st" -- on
expect_eq "compression on writes on and owner hook" "on:hook" "$(cat "$st/compression"):$(cat "$st/owner")"
expect_eq "compression on keeps the state files private" "700:600:600" \
  "$(mode "$st"):$(mode "$st/compression"):$(mode "$st/owner")"
expect_eq "compression on leaves no temp file" "" "$(find "$st" -mindepth 1 -name ".*")"
expect_eq "hook with compression on runs with --minify-json" "fake-deslag hook claude --minify-json" \
  "$(run_hook_env "$data" DESLAG_STATE_DIR="$st" </dev/null)"
expect_eq "hook with compression on passes stdin to the binary" \
  "$(printf 'fake-deslag hook claude --minify-json\n{"tool_name":"Bash"}')" \
  "$(echo '{"tool_name":"Bash"}' | run_hook_env "$data" DESLAG_STATE_DIR="$st")"
out=$(run_compression DESLAG_STATE_DIR="$st" -- status "$data")
expect_match "compression status reports on and owner hook" "compression: on.*owner: hook" "$(tr '\n' ' ' <<<"$out")"
expect_match "compression status with a data dir reports the binary" "installed at" "$out"
expect_match "compression status without the binary points to /deslag:install" "missing.*/deslag:install" \
  "$(run_compression DESLAG_STATE_DIR="$st" -- status "$tmp/data-none" | tr '\n' ' ')"
expect_match "an empty subcommand is status" "compression: on" "$(run_compression DESLAG_STATE_DIR="$st" -- "")"

echo proxy >"$st/owner"
expect_match "compression on but owner proxy is noted" "owner is proxy" "$(run_compression DESLAG_STATE_DIR="$st" -- status)"
expect_match "compression on from owner proxy reports two adapters and a new session" "two adapters.*Start a new Claude Code session" \
  "$(run_compression DESLAG_STATE_DIR="$st" -- on)"
expect_eq "compression on replaces owner proxy with hook" hook "$(cat "$st/owner")"
# The owner rule is session::read_owner: the whole file, trailing whitespace stripped.
for owner in 'hook\n' 'hook' 'hook \n' 'hook\r\n'; do
  printf %b "$owner" >"$st/owner"
  expect_match "compression status reads owner $owner as hook" "owner: hook" "$(run_compression DESLAG_STATE_DIR="$st" -- status)"
done
for owner in 'hook\nproxy\n' 'none\n'; do
  printf %b "$owner" >"$st/owner"
  expect_match "compression status reads owner $owner as none" "owner: none.*hook does not compress" \
    "$(run_compression DESLAG_STATE_DIR="$st" -- status | tr '\n' ' ')"
done
printf 'proxy \r\n' >"$st/owner"
expect_match "compression on from owner 'proxy \\r\\n' reports two adapters" "two adapters" \
  "$(run_compression DESLAG_STATE_DIR="$st" -- on)"

expect_ok "compression off succeeds" run_compression DESLAG_STATE_DIR="$st" -- off
expect_eq "compression off writes off and keeps the owner" "off:hook" "$(cat "$st/compression"):$(cat "$st/owner")"
expect_match "compression status reports off" "compression: off" "$(run_compression DESLAG_STATE_DIR="$st" -- status)"
expect_eq "hook with compression off runs without --minify-json" "fake-deslag hook claude" \
  "$(run_hook_env "$data" DESLAG_STATE_DIR="$st" </dev/null)"
printf 'on \n' >"$st/compression"
expect_eq "hook treats a first line other than on as off" "fake-deslag hook claude" \
  "$(run_hook_env "$data" DESLAG_STATE_DIR="$st" </dev/null)"
printf 'on' >"$st/compression"
expect_eq "hook accepts on without a trailing newline" "fake-deslag hook claude --minify-json" \
  "$(run_hook_env "$data" DESLAG_STATE_DIR="$st" </dev/null)"

# Empty and relative values are skipped, like deslag_telemetry::state_dir.
expect_ok "compression on skips a relative DESLAG_STATE_DIR" \
  run_compression DESLAG_STATE_DIR=rel XDG_STATE_HOME="$tmp/xdg" -- on
expect_eq "compression on falls back to XDG_STATE_HOME/deslag" on "$(cat "$tmp/xdg/deslag/compression")"
expect_eq "hook skips a relative DESLAG_STATE_DIR" "fake-deslag hook claude --minify-json" \
  "$(run_hook_env "$data" DESLAG_STATE_DIR=rel XDG_STATE_HOME="$tmp/xdg" </dev/null)"
expect_ok "compression on skips empty and relative values" \
  run_compression DESLAG_STATE_DIR= XDG_STATE_HOME=rel HOME="$tmp/home" -- on
expect_eq "compression on falls back to HOME/.local/state/deslag" on "$(cat "$tmp/home/.local/state/deslag/compression")"
expect_eq "hook skips empty and relative values" "fake-deslag hook claude --minify-json" \
  "$(run_hook_env "$data" DESLAG_STATE_DIR= XDG_STATE_HOME=rel HOME="$tmp/home" </dev/null)"
expect_fail "compression without a state dir fails" "no state directory" run_compression DESLAG_STATE_DIR=rel -- on
: >"$tmp/state-file"
expect_fail "compression on refuses a state dir that is a file" "not a directory" \
  run_compression DESLAG_STATE_DIR="$tmp/state-file" -- on
mkdir -p "$tmp/state-dirfile/compression"
expect_fail "compression on refuses a state file that is a directory" "not a file" \
  run_compression DESLAG_STATE_DIR="$tmp/state-dirfile" -- on
expect_eq "compression on leaves the directory and no temp file" "compression" "$(ls -A "$tmp/state-dirfile")"
expect_fail "compression rejects an unknown subcommand" "usage" run_compression DESLAG_STATE_DIR="$st" -- frobnicate

# With compression on, the installed binary's `status` conflict lines are shown.
cst=$tmp/state-conflict
cdata=$tmp/data-conflict
mkdir -p "$cdata/bin/$version"
# shellcheck disable=SC2016 # the stub expands $* and $(cat) at run time
printf '#!/bin/sh\necho "deslag status line"\necho "conflict: stub $*"\necho "conflict fix: stub fix"\necho "conflict: stdin=[$(cat)]"\n' >"$cdata/bin/$version/deslag"
chmod 755 "$cdata/bin/$version/deslag"
out=$(run_compression DESLAG_STATE_DIR="$cst" -- on "$cdata" </dev/null)
expect_match "compression on prints the binary's conflict lines" "conflict: stub status.*conflict fix: stub fix" "$(tr '\n' ' ' <<<"$out")"
expect_eq "compression on prints only the conflict lines of the binary" absent \
  "$(grep -q 'deslag status line' <<<"$out" && echo present || echo absent)"
out=$(echo stdin | run_compression DESLAG_STATE_DIR="$cst" -- status "$cdata")
expect_match "compression status prints the binary's conflict lines" "conflict: stub status" "$out"
expect_eq "compression status passes no stdin to the binary" "conflict: stdin=[]" "$(grep '^conflict: stdin=' <<<"$out")"
expect_eq "compression status without a binary prints no conflict line" absent \
  "$(run_compression DESLAG_STATE_DIR="$cst" -- status "$tmp/data-none" | grep -q '^conflict' && echo present || echo absent)"
expect_eq "compression on without a binary prints no conflict line" absent \
  "$(run_compression DESLAG_STATE_DIR="$cst" -- on "$tmp/data-none" | grep -q '^conflict' && echo present || echo absent)"
# A plugin copy without plugin.json still exits 0 and prints no conflict line.
cp -R "$root" "$tmp/plugin-nojson"
command rm -f "$tmp/plugin-nojson/.claude-plugin/plugin.json"
if out=$(env -i PATH=/usr/bin:/bin DESLAG_STATE_DIR="$cst" "$tmp/plugin-nojson/scripts/compression.sh" status "$cdata" 2>/dev/null); then
  ok "compression status without plugin.json exits 0"
else
  not_ok "compression status without plugin.json exits 0" "$out"
fi
expect_eq "compression status without plugin.json prints no conflict line" absent \
  "$(grep -q '^conflict' <<<"$out" && echo present || echo absent)"
out=$(run_compression DESLAG_STATE_DIR="$cst" -- off "$cdata")
expect_eq "compression off prints no conflict line" absent \
  "$(grep -q '^conflict' <<<"$out" && echo present || echo absent)"

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
    '!`"${CLAUDE_PLUGIN_ROOT}/scripts/install.sh" '"$c"' "${CLAUDE_PLUGIN_DATA}"`' \
    "$(grep '^!`' "$root/commands/$c.md")"
  # The quoted rule matches the quoted command even when the plugin root has a space.
  # shellcheck disable=SC2016 # the literal placeholder is expected
  expect_eq "commands/$c.md allows only install.sh" \
    'allowed-tools: Bash("${CLAUDE_PLUGIN_ROOT}/scripts/install.sh":*)' \
    "$(grep '^allowed-tools:' "$root/commands/$c.md")"
done
# shellcheck disable=SC2016 # the literal placeholders are expected
expect_eq "commands/compression.md passes the arguments and the data dir" \
  '!`"${CLAUDE_PLUGIN_ROOT}/scripts/compression.sh" "$ARGUMENTS" "${CLAUDE_PLUGIN_DATA}"`' \
  "$(grep '^!`' "$root/commands/compression.md")"
# shellcheck disable=SC2016 # the literal placeholder is expected
expect_eq "commands/compression.md allows only compression.sh" \
  'allowed-tools: Bash("${CLAUDE_PLUGIN_ROOT}/scripts/compression.sh":*)' \
  "$(grep '^allowed-tools:' "$root/commands/compression.md")"

# --- uninstall -----------------------------------------------------------------
expect_ok "uninstall succeeds" run_install "$good" "$data" uninstall
expect_eq "uninstall removes the data dir" "absent" "$([ -e "$data" ] && echo present || echo absent)"
mkdir -p "$tmp/foreign" && echo keep >"$tmp/foreign/file"
expect_fail "uninstall refuses a dir without the marker" "refus" run_install "$good" "$tmp/foreign" uninstall
expect_eq "refused uninstall keeps the dir" keep "$(cat "$tmp/foreign/file")"
expect_ok "uninstall of a missing data dir is a no-op" run_install "$good" "$tmp/data-gone" uninstall
# The client creates the data dir on demand, so an empty one without a marker is fine.
mkdir -p "$tmp/data-blank"
expect_match "uninstall of an empty unmarked dir reports nothing installed" "nothing installed" \
  "$(run_install "$good" "$tmp/data-blank" uninstall)"
expect_ok "uninstall of an empty unmarked dir exits 0" run_install "$good" "$tmp/data-blank" uninstall
mkdir -p "$tmp/linked-udata/bin" && : >"$tmp/linked-udata/.deslag-data" && echo keep >"$tmp/linked-udata/bin/file"
ln -s "$tmp/linked-udata" "$tmp/data-ulink"
expect_fail "uninstall refuses a symlinked data dir" "symlink" run_install "$good" "$tmp/data-ulink" uninstall
expect_fail "uninstall refuses a data dir with a . component" "components" run_install "$good" "$tmp/data-ulink/." uninstall
expect_eq "refused uninstall keeps the link target" keep "$(cat "$tmp/linked-udata/bin/file")"
expect_eq "refused uninstall keeps the target marker" present "$([ -f "$tmp/linked-udata/.deslag-data" ] && echo present || echo absent)"
# Content that install.sh did not create stops uninstall after its own files are gone.
run_install "$good" "$tmp/data-extra" install >/dev/null
echo keep >"$tmp/data-extra/user-file"
expect_fail "uninstall leaves a data dir with unknown content" "did not create" run_install "$good" "$tmp/data-extra" uninstall
expect_eq "uninstall keeps unknown content" keep "$(cat "$tmp/data-extra/user-file")"
expect_eq "uninstall still removes what install created" "user-file" "$(ls -A "$tmp/data-extra")"
# The README manual-copy path: copy the binary, then touch the marker.
mkdir -p "$tmp/data-manual/bin/$version" && printf '#!/bin/sh\n' >"$tmp/data-manual/bin/$version/deslag"
touch "$tmp/data-manual/.deslag-data"
expect_ok "uninstall removes a manually copied binary with the marker" run_install "$good" "$tmp/data-manual" uninstall
expect_eq "manual-copy uninstall removes the data dir" absent "$([ -e "$tmp/data-manual" ] && echo present || echo absent)"

if [ "$failures" -ne 0 ]; then echo "$failures test(s) failed" >&2; exit 1; fi
echo "all plugin tests passed"
