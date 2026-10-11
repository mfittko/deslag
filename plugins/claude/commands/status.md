---
description: Show whether the deslag binary for this plugin version is installed
disable-model-invocation: true
allowed-tools: Bash("${CLAUDE_PLUGIN_ROOT}/scripts/install.sh":*)
---

!`"${CLAUDE_PLUGIN_ROOT}/scripts/install.sh" status "${CLAUDE_PLUGIN_DATA}"`

Report the command output above to the user verbatim. Do not run other commands.
