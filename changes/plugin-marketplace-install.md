### Added

- The repository is a Claude Code plugin marketplace: `/plugin marketplace add mfittko/deslag` lists the `deslag` plugin (#40)
- `/deslag:install` downloads the `deslag` release archive for the plugin version, verifies it against `SHA256SUMS` and installs it into the private plugin data directory; `/deslag:status` and `/deslag:uninstall` report on and remove it (#40)

### Changed

- The plugin hook runs the binary installed by `/deslag:install` and no longer needs `deslag` on `PATH`; without a matching binary it leaves tool results unchanged (#40)
