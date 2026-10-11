#!/bin/sh
# PostToolUse hook wrapper. Runs the deslag binary installed for this plugin
# version by /deslag:install. When that binary is missing, it reads and discards
# the hook input, then exits 0 without output, which leaves the tool result
# unchanged. Reading the input keeps the client from a broken pipe on large tool
# results. It never downloads anything.
pass() {
  # ponytail: cat drains megabytes at once; a shell read loop takes minutes for 1 MiB.
  cat >/dev/null 2>&1
  exit 0
}
if [ -z "${CLAUDE_PLUGIN_ROOT:-}" ] || [ -z "${CLAUDE_PLUGIN_DATA:-}" ]; then pass; fi
version=
while IFS= read -r line || [ -n "$line" ]; do
  case $line in
    *'"version"'*)
      line=${line#*'"version"'}
      line=${line#*'"'}
      version=${line%%'"'*}
      break
      ;;
  esac
done 2>/dev/null <"$CLAUDE_PLUGIN_ROOT/.claude-plugin/plugin.json"
bin=$CLAUDE_PLUGIN_DATA/bin/$version/deslag
if [ -z "$version" ] || [ ! -f "$bin" ] || [ ! -x "$bin" ]; then pass; fi
exec "$bin" hook claude
