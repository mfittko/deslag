### Added

- The local service answers `GET /session/<id>` on `local-http.sock` with the `deslag stats --json --session <id>` document of the last 24 hours (#91)
- The Claude Code usage module has an opt-in savings widget. With `DESLAG_SAVINGS_WIDGET=1`, the status line shows the estimated tokens removed from each model request next to its provider input tokens, labeled as an estimate (#91)
- `docs/demo.md` walks through hook compression and the savings widget in a throwaway session (#91)
