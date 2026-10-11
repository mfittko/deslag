# Changelog fragments

A pull request that changes user-visible behavior adds one new fragment file
here and leaves `CHANGELOG.md` unchanged. Unique file names keep concurrent pull
requests from conflicting on the changelog.

## Adding a fragment

Create `changes/<slug>.md` with a unique slug, for example
`changes/proxy-timeouts.md`. The file must be a non-empty regular file, and its name must not start with
a dot because `bump.sh` skips dotfiles.

- A section line (`### Added`, `### Changed` or `### Fixed`) may appear
  anywhere. It switches the section for the entries that follow. Entries
  before any section line go under `### Changed`.
- Every other non-blank line is a Keep-a-Changelog entry:
  `- <user-visible effect> (#<issue-or-PR>)`.
- Any other heading or line fails the check.

Example:

```markdown
### Fixed

- The proxy keeps request paths inside the configured base path (#35)
```

## The CI check

The `changelog` workflow fails a pull request that changes files under
`crates/` or `plugins/` and adds, renames or modifies no fragment. It also
fails when an added, renamed or modified fragment breaks the rules above or has
no entry; `bump.sh` applies the same rules. A change with no user-visible
effect opts out with this pull request body line. The check matches the line trimmed
and case-insensitively:

```text
changelog: none
```

Editing the pull request body re-runs the check.

## Release assembly

`scripts/release/bump.sh <version>` folds every fragment into the
`## Unreleased` section of `CHANGELOG.md` under `### Added`, `### Changed` and
`### Fixed`, in that order. Existing Unreleased entries come first. It removes
the consumed fragments and stamps the section as `## <version> - <date>`. This
`README.md` is never a fragment.
