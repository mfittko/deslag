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
#   uninstall  Remove what install created, then the data directory if it is
#              empty.
# DESLAG_RELEASE_BASE_URL overrides the release download base (tests use file://).
set -euo pipefail
export LC_ALL=C
umask 077

die() { echo "error: $*" >&2; exit 1; }
usage="usage: install.sh <install|status|uninstall> <data-dir>"
case "${1:-}" in install | status | uninstall) ;; *) die "$usage" ;; esac

root=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
data=${2:-${CLAUDE_PLUGIN_DATA:-}}
[ -n "$data" ] || die "no data dir: pass it as the second argument or set CLAUDE_PLUGIN_DATA; run this through the deslag plugin commands"
[[ $data == /* ]] || die "the data dir must be an absolute path: $data"
# A . or .. component would hide a symlinked data dir from the symlink checks below.
[[ /$data/ != */./* && /$data/ != */../* ]] || die "the data dir must not contain . or .. components: $data"
# Strip trailing slashes so the symlink checks below see the link itself.
while [[ $data == */ && $data != / ]]; do data=${data%/}; done
marker=$data/.deslag-data
version=$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$root/.claude-plugin/plugin.json" | head -n 1)
[ -n "$version" ] || die "no version in $root/.claude-plugin/plugin.json"
bin=$data/bin/$version/deslag
fix="Run /deslag:install to install deslag $version."

stage=
trap '[ -z "$stage" ] || command rm -rf "$stage"' EXIT

# has_bin <dir>: <dir> holds an executable regular deslag file.
has_bin() { [ -f "$1/deslag" ] && [ -x "$1/deslag" ]; }

install() {
  local target base proto name sum expected actual dir
  case "$(uname -s)/$(uname -m)" in
    Darwin/arm64) target=aarch64-apple-darwin ;;
    Linux/x86_64) target=x86_64-unknown-linux-gnu ;;
    *) die "no deslag release for $(uname -s) $(uname -m); build it with cargo build --release -p deslag-cli and copy target/release/deslag to $bin, then run touch $marker so that uninstall removes it" ;;
  esac
  # mkdir, chmod, the marker write, the mv onto $bin and the prune loop would
  # follow a symlink out of the data dir, so refuse each symlinked path before
  # touching anything.
  for dir in "$data" "$data/bin" "$data/bin/$version" "$bin" "$marker"; do
    [ ! -L "$dir" ] || die "refusing to install: $dir is a symlink"
  done
  base=${DESLAG_RELEASE_BASE_URL:-https://github.com/mfittko/deslag/releases/download}
  proto='=https'
  [ -z "${DESLAG_RELEASE_BASE_URL:-}" ] || proto='=https,file'
  name=deslag-$version-$target

  mkdir -p "$data/bin"
  chmod 700 "$data" "$data/bin"
  : >"$marker"
  stage=$(mktemp -d "$data/.stage.XXXXXX")

  for file in "$name.tar.gz" SHA256SUMS; do
    curl -fsSL --connect-timeout 15 --max-time 300 --proto "$proto" --proto-redir "$proto" -o "$stage/$file" "$base/v$version/$file" ||
      die "download failed: $base/v$version/$file"
  done
  expected=$(awk -v f="$name.tar.gz" '$2 == f || $2 == "*" f { print $1; exit }' "$stage/SHA256SUMS")
  [ -n "$expected" ] || die "no checksum for $name.tar.gz in SHA256SUMS"
  if command -v sha256sum >/dev/null; then sum=(sha256sum); else sum=(shasum -a 256); fi
  actual=$("${sum[@]}" "$stage/$name.tar.gz" | awk '{ print $1 }')
  [ "$actual" = "$expected" ] || die "checksum mismatch for $name.tar.gz; nothing installed"

  tar -xzf "$stage/$name.tar.gz" -C "$stage" "$name/deslag" || die "archive has no $name/deslag"
  if [ ! -f "$stage/$name/deslag" ] || [ -L "$stage/$name/deslag" ]; then
    die "archive has no regular file $name/deslag"
  fi
  chmod 700 "$stage/$name/deslag"
  mkdir -p "$data/bin/$version"
  chmod 700 "$data/bin/$version"
  command mv -f "$stage/$name/deslag" "$bin"
  # No trailing slash: rm removes a symlinked version dir as a link and never
  # follows it into the target.
  for dir in "$data"/bin/*; do
    [ "$dir" = "$data/bin/$version" ] || command rm -rf "$dir"
  done
  echo "Installed deslag $version for $target at $bin"
}

status() {
  local found='' dir
  for dir in "$data"/bin/*/; do
    ! has_bin "$dir" || found="$found $(basename "$dir")"
  done
  if has_bin "$data/bin/$version"; then
    echo "deslag $version: installed at $bin"
  elif [ -n "$found" ]; then
    echo "deslag $version: version mismatch (installed:$found). The hook is inactive. $fix"
  else
    echo "deslag $version: missing. The hook is inactive. $fix"
  fi
}

uninstall() {
  # Removing through a symlinked data dir would delete files in its target.
  [ ! -L "$data" ] || die "refusing to uninstall: $data is a symlink"
  if [ ! -e "$data" ]; then
    echo "Nothing to remove: $data does not exist."
    return
  fi
  if [ ! -f "$marker" ]; then
    # The client creates the data dir on demand, so an empty one is not ours.
    if [ -d "$data" ] && [ -z "$(ls -A "$data")" ]; then
      echo "Nothing installed in $data."
      return
    fi
    die "refusing to remove $data: it has no $(basename "$marker") marker"
  fi
  # Remove only what install creates; rmdir fails on anything else.
  command rm -rf "$data/bin" "$data"/.stage.*
  command rm -f "$marker"
  rmdir "$data" || die "left $data in place: it holds files that install.sh did not create"
  echo "Removed $data. claude plugin uninstall also removes it."
}

"$1"
