### Added

- `/deslag:compression on|off|status` toggles hook compression for the Claude Code plugin; `on` writes `on` to `<state dir>/compression` and `hook` to `<state dir>/owner`, and the plugin hook then runs `deslag hook claude --minify-json`; the default stays off (#20)
- The savings widget line ends with the hook compression state, `compression on` or `compression off` (#20)
- If you added a project hook that runs `deslag hook claude --minify-json` by hand, remove it; with the toggle on, both that hook and the plugin hook compress
