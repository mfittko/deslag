---
description: Turn deslag hook compression on or off, or show its state
argument-hint: on | off | status
disable-model-invocation: true
allowed-tools: Bash("${CLAUDE_PLUGIN_ROOT}/scripts/compression.sh":*)
---

!`"${CLAUDE_PLUGIN_ROOT}/scripts/compression.sh" "$ARGUMENTS" "${CLAUDE_PLUGIN_DATA}"`

Report the command output above to the user verbatim. Do not run other commands.
