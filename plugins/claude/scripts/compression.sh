#!/usr/bin/env bash
# Usage: compression.sh [on|off|status] [<data-dir>]
# Toggles hook compression for the plugin hook. The choice lives in
# <state dir>/compression; the plugin hook passes --minify-json only while its
# first line is exactly `on`. A missing or unknown value is off.
#   on      Write `on` and make the hook the compression owner (<state dir>/owner).
#   off     Write `off`. The owner stays unchanged.
#   status  Report the toggle, the owner and, with a data dir, whether the
#           version-matched binary is installed. With compression on and that
#           binary installed, also the `conflict` lines of `deslag status`.
#           An empty subcommand is status.
# The state dir rule is deslag_telemetry::state_dir: DESLAG_STATE_DIR, else
# $XDG_STATE_HOME/deslag, else $HOME/.local/state/deslag. Empty and relative
# values are skipped. <data-dir> falls back to $CLAUDE_PLUGIN_DATA.
set -euo pipefail
export LC_ALL=C
umask 077

die() { echo "error: $*" >&2; exit 1; }
cmd=${1:-status}
case $cmd in on | off | status) ;; *) die "usage: compression.sh [on|off|status] [<data-dir>]" ;; esac

root=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
data=${2:-${CLAUDE_PLUGIN_DATA:-}}
dir=
for v in "${DESLAG_STATE_DIR:-}" "${XDG_STATE_HOME:+$XDG_STATE_HOME/deslag}" "${HOME:+$HOME/.local/state/deslag}"; do
  if [[ $v == /* ]]; then dir=$v; break; fi
done
[ -n "$dir" ] || die "no state directory: set DESLAG_STATE_DIR, XDG_STATE_HOME or HOME"

tmp=
trap '[ -z "$tmp" ] || command rm -f "$tmp"' EXIT

# first_line <file>: the first line, or empty when the file is missing.
first_line() { head -n 1 -- "$1" 2>/dev/null || true; }

# read_owner: the owner as session::read_owner reads it. The whole
# <state dir>/owner file with trailing whitespace (CR included) stripped must be
# exactly `hook` or `proxy`; anything else, a missing file included, is none.
read_owner() {
  local text
  text=$(cat -- "$dir/owner" 2>/dev/null || true)
  text=${text%"${text##*[![:space:]]}"}
  case $text in hook | proxy) echo "$text" ;; *) echo none ;; esac
}

# put <name> <value>: atomically replace <state dir>/<name> with a 0600 file.
put() {
  [ ! -d "$dir/$1" ] || die "not a file: $dir/$1"
  tmp=$(mktemp "$dir/.$1.XXXXXX")
  printf '%s\n' "$2" >"$tmp"
  chmod 600 "$tmp"
  command mv -f "$tmp" "$dir/$1"
  tmp=
}

# Same checks as deslag_telemetry::ensure_state_dir.
ensure_dir() {
  [ ! -e "$dir" ] || [ -d "$dir" ] || die "the state dir is not a directory: $dir"
  mkdir -p "$dir"
  [ -O "$dir" ] || die "the state dir is not owned by the current user: $dir"
  chmod 700 "$dir"
}

status() {
  local state owner
  state=$(first_line "$dir/compression")
  [ "$state" = on ] || state=off
  owner=$(read_owner)
  echo "deslag compression: $state (state dir: $dir)"
  echo "owner: $owner"
  if [ "$state" = on ] && [ "$owner" != hook ]; then
    echo "note: the owner is $owner, so the hook does not compress. Run /deslag:compression on to make the hook the owner."
  fi
  [ -z "$data" ] || "$root/scripts/install.sh" status "$data" || true
  [ "$state" = on ] && [ -n "$data" ] || return 0
  local version bin
  version=$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$root/.claude-plugin/plugin.json" | head -n 1)
  bin=$data/bin/$version/deslag
  if [ -n "$version" ] && [ -f "$bin" ] && [ -x "$bin" ]; then
    "$bin" status </dev/null 2>/dev/null | grep '^conflict' || true
  fi
}

on() {
  local previous
  ensure_dir
  previous=$(read_owner)
  put compression on
  put owner hook
  echo "Turned hook compression on. It applies to new tool results."
  if [ "$previous" = proxy ]; then
    echo "The owner changed from proxy to hook. The current session now has two adapters: the proxy replays its pinned results and the hook compresses new ones. Start a new Claude Code session."
  fi
}

off() {
  ensure_dir
  put compression off
  echo "Turned hook compression off. New tool results stay unchanged; results already compressed in this session stay compressed."
}

case $cmd in
  on) on ;;
  off) off ;;
esac
status
