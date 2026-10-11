#!/bin/sh
# PostToolUse hook wrapper. Runs the deslag binary installed for this plugin
# version by /deslag:install. When that binary is missing, it exits 0 without
# output, which leaves the tool result unchanged. It uses shell builtins only
# and never downloads anything.
[ -n "${CLAUDE_PLUGIN_ROOT:-}" ] && [ -n "${CLAUDE_PLUGIN_DATA:-}" ] || exit 0
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
[ -n "$version" ] && [ -x "$bin" ] || exit 0
exec "$bin" hook claude
