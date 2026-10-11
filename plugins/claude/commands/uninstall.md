---
description: Remove the deslag binary and the plugin data directory
disable-model-invocation: true
allowed-tools: Bash(${CLAUDE_PLUGIN_ROOT}/scripts/install.sh:*)
---

!`${CLAUDE_PLUGIN_ROOT}/scripts/install.sh uninstall`

Report the command output above to the user verbatim. Do not run other commands.
