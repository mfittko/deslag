#!/usr/bin/env bash
# Usage: install.sh <install|status|uninstall> <data-dir>
# Manages the version-matched deslag binary in the plugin data directory
# (<data-dir>/bin/<version>/deslag). It never touches PATH or any binary
# outside that directory.
# The plugin root is the parent of this script's directory. <data-dir> falls
# back to $CLAUDE_PLUGIN_DATA. Plugin commands pass "${CLAUDE_PLUGIN_DATA}"
# because the client substitutes it in command text but does not export it.
#   install    Download the release archive for the plugin version and this
#              platform, verify it against SHA256SUMS and install it.
#   status     Report whether the matching binary is installed.
#   uninstall  Remove the data directory.
# DESLAG_RELEASE_BASE_URL overrides the release download base (tests use file://).
set -euo pipefail
export LC_ALL=C
umask 077

die() { echo "error: $*" >&2; exit 1; }

root=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
data=${2:-${CLAUDE_PLUGIN_DATA:-}}
[ -n "$data" ] || die "no data dir: pass it as the second argument or set CLAUDE_PLUGIN_DATA; run this through the deslag plugin commands"
[[ $data == /* ]] || die "the data dir must be an absolute path: $data"
marker=$data/.deslag-data
version=$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$root/.claude-plugin/plugin.json" | head -n 1)
[ -n "$version" ] || die "no version in $root/.claude-plugin/plugin.json"
bin=$data/bin/$version/deslag
fix="Run /deslag:install to install deslag $version."

install() {
  local target base proto name stage sum expected actual
  case "$(uname -s)/$(uname -m)" in
    Darwin/arm64) target=aarch64-apple-darwin ;;
    Linux/x86_64) target=x86_64-unknown-linux-gnu ;;
    *) die "no deslag release for $(uname -s) $(uname -m); build it with cargo install --path crates/deslag-cli --locked" ;;
  esac
  base=${DESLAG_RELEASE_BASE_URL:-https://github.com/mfittko/deslag/releases/download}
  proto='=https'
  [ -z "${DESLAG_RELEASE_BASE_URL:-}" ] || proto='=https,file'
  name=deslag-$version-$target

  mkdir -p "$data/bin"
  chmod 700 "$data" "$data/bin"
  : >"$marker"
  stage=$(mktemp -d "$data/.stage.XXXXXX")
  # shellcheck disable=SC2064 # expand $stage now
  trap "command rm -rf '$stage'" EXIT

  for file in "$name.tar.gz" SHA256SUMS; do
    curl -fsSL --proto "$proto" --proto-redir "$proto" -o "$stage/$file" "$base/v$version/$file" ||
      die "download failed: $base/v$version/$file"
  done
  expected=$(awk -v f="$name.tar.gz" '$2 == f || $2 == "*" f { print $1; exit }' "$stage/SHA256SUMS")
  [ -n "$expected" ] || die "no checksum for $name.tar.gz in SHA256SUMS"
  if command -v sha256sum >/dev/null; then sum=(sha256sum); else sum=(shasum -a 256); fi
  actual=$("${sum[@]}" "$stage/$name.tar.gz" | awk '{ print $1 }')
  [ "$actual" = "$expected" ] || die "checksum mismatch for $name.tar.gz; nothing installed"

  tar -xzf "$stage/$name.tar.gz" -C "$stage" "$name/deslag" || die "archive has no $name/deslag"
  [ -f "$stage/$name/deslag" ] && [ ! -L "$stage/$name/deslag" ] || die "archive has no regular file $name/deslag"
  chmod 700 "$stage/$name/deslag"
  mkdir -p "$data/bin/$version"
  chmod 700 "$data/bin/$version"
  command mv -f "$stage/$name/deslag" "$bin"
  for dir in "$data"/bin/*/; do
    [ "$dir" = "$data/bin/$version/" ] || command rm -rf "$dir"
  done
  echo "Installed deslag $version for $target at $bin"
}

status() {
  local found=
  for dir in "$data"/bin/*/; do
    [ -d "$dir" ] && found="$found $(basename "$dir")"
  done
  if [ -x "$bin" ]; then
    echo "deslag $version: installed at $bin"
  elif [ -n "$found" ]; then
    echo "deslag $version: version mismatch (installed:$found). The hook is inactive. $fix"
  else
    echo "deslag $version: missing. The hook is inactive. $fix"
  fi
}

uninstall() {
  if [ ! -e "$data" ]; then
    echo "Nothing to remove: $data does not exist."
    return
  fi
  [ -f "$marker" ] || die "refusing to remove $data: it has no $(basename "$marker") marker"
  command rm -rf "$data"
  echo "Removed $data. claude plugin uninstall also removes it."
}

case "${1:-install}" in
  install) install ;;
  status) status ;;
  uninstall) uninstall ;;
  *) die "usage: install.sh <install|status|uninstall> <data-dir>" ;;
esac
