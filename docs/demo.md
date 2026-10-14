# Demo: savings widget in Claude Code

This guide runs hook compression in a throwaway Claude Code session and shows the savings widget. The widget shows, per model request, the tokens that hook compression removed next to the input tokens the provider reported. The removed count comes from the local `deslag-estimate-v1` estimator, so the widget labels it as an estimate. The input count is provider-reported usage. The session figure totals the removed tokens of the session's requests in the last 24 hours. The widget shows no money figures.

## Requirements

- A checkout of this repository and a Rust toolchain.
- Claude Code with mod modules (`CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1`).

## 1. Build the binary

From the repository root:

```sh
cargo build --release -p deslag-cli
export DESLAG_BIN="$PWD/target/release/deslag"
export DESLAG_PLUGIN="$PWD/plugins/claude"
```

## 2. Use a throwaway state directory

The state directory holds the owner file, the service sockets and the stats files. macOS limits socket paths to 104 bytes, so keep the path short.

```sh
export DESLAG_STATE_DIR="$(mktemp -d /tmp/deslag-demo.XXXXXX)"
echo hook > "$DESLAG_STATE_DIR/owner"
"$DESLAG_BIN" local start
```

The owner file `hook` makes the hook the compression owner. `deslag local start` starts the stats service in the background. Export `DESLAG_STATE_DIR` in the shell that starts Claude Code too, so the hook, the usage module and the service share one directory.

## 3. Enable hook compression

The plugin's own hook (`hooks/deslag-hook.sh`) does not pass `--minify-json`, so it leaves tool results unchanged. Add a project hook that runs the binary with the flag. Create a throwaway project directory with its own settings file:

```sh
mkdir -p /tmp/deslag-demo-project/.claude
cd /tmp/deslag-demo-project
cat > .claude/settings.json <<EOF
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "*",
        "hooks": [
          { "type": "command", "command": "$DESLAG_BIN hook claude --minify-json" }
        ]
      }
    ]
  }
}
EOF
```

The heredoc writes the absolute binary path into the file. deslag's hook conflict scan treats `<path>/deslag hook claude --minify-json` as its own hook, so the two hooks do not report a conflict.

## 4. Create JSON-heavy input

The hook compresses a result only when it removes at least 64 estimated tokens and at least 10% of the estimated tokens. Pretty-printed JSON clears both thresholds:

```sh
{
  echo '['
  for i in $(seq 1 200); do
    sep=','
    [ "$i" -eq 200 ] && sep=''
    printf '  {\n    "id": %d,\n    "name": "item-%d",\n    "status": "active",\n    "tags": [\n      "alpha",\n      "beta"\n    ]\n  }%s\n' "$i" "$i" "$sep"
  done
  echo ']'
} > items.json
```

## 5. Start Claude Code with the plugin and the widget

```sh
CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 DESLAG_USAGE_STATS=1 DESLAG_SAVINGS_WIDGET=1 \
  claude --plugin-dir "$DESLAG_PLUGIN"
```

- `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1` loads the plugin's usage module.
- `DESLAG_USAGE_STATS=1` sends the provider usage of each model request to the service.
- `DESLAG_SAVINGS_WIDGET=1` shows the savings line after each model request.

Ask Claude:

```text
Run `cat items.json` with the Bash tool and tell me how many items it has.
```

The model request that reads the tool result takes the hook's report. After that request, the status line shows a line of this shape:

```text
deslag: ~1,234 tokens removed (estimate) of 5,678 input tokens · session ~1,234 removed (estimate)
```

`unknown` means that a count was not available. A 0 is a reported count of 0. When the service is not running, the widget shows no line.

## 6. Check the stats

Write the open requests and read the session ID from the last request line:

```sh
"$DESLAG_BIN" stats --json > /dev/null
tail -n 1 "$DESLAG_STATE_DIR/requests.jsonl"
"$DESLAG_BIN" stats --json --session <session id>
```

At least one request in the output has a non-zero `estimated_removed_tokens`.

## 7. Clean up

```sh
"$DESLAG_BIN" local stop
rm -rf "$DESLAG_STATE_DIR" /tmp/deslag-demo-project
```

## Recorded run

This run used Claude Code 2.1.299 on macOS arm64 on 2026-10-14. It followed steps 1 to 4 and then ran one headless request with `claude -p --model haiku --max-budget-usd 0.30` and the step 5 variables, with `--allowedTools "Bash(cat items.json)"`. The request cost USD 0.0094. Headless mode draws no status line. A throwaway probe plugin hooked `ui.status` and logged each line the widget set. These are the two lines, one per model request:

```text
deslag: ~0 tokens removed (estimate) of 38,349 input tokens · session ~0 removed (estimate)
deslag: ~3,603 tokens removed (estimate) of 55,388 input tokens · session ~3,603 removed (estimate)
```

The first request runs the Bash tool. The second request reads the compressed tool result, so it takes the hook's report. `items.json` is 23,787 bytes. The model answered that the file has 200 items.

`deslag stats --json --session <session id>` showed the same figures (fields shortened):

```json
{"index":0,"compression_reports":0,"estimated_removed_tokens":0,"usage":{"input_tokens":2,"cache_read_input_tokens":11447,"cache_creation_input_tokens":26900,"output_tokens":79}}
{"index":1,"compression_reports":1,"estimated_removed_tokens":3603,"usage":{"input_tokens":2,"cache_read_input_tokens":38347,"cache_creation_input_tokens":17039,"output_tokens":136}}
```

The session totals were 2 requests and an `estimated_removed_tokens` of 3603.
