# Release runbook

A release is a `v<version>` tag on a `main` commit. The tag push runs
`.github/workflows/release.yml`, which builds, attests and publishes the GitHub
Release. Nothing else publishes artifacts. crates.io, Homebrew and container
images are out of scope.

## Version surfaces

Two files carry the version, and they must always agree:

- `Cargo.toml`: `[workspace.package] version`
- `plugins/claude/.claude-plugin/plugin.json`: `version`

`scripts/release/check-versions.sh` fails when they differ. CI runs it on every
pull request and push. The release workflow also fails unless the tag equals
both surfaces.

## Tooling

The release scripts in `scripts/release/` are bash with `jq`, `awk` and `git`.
They need no Node or Python and run on GNU and BSD (macOS) userland.
`scripts/release/test.sh` tests them; CI runs it together with `shellcheck`.

| Script | Purpose |
| --- | --- |
| `check-versions.sh [<tag>]` | Print the version; fail on a surface mismatch or a tag mismatch. |
| `check-changelog-fragment.sh <base>` | Fail a `crates/` or `plugins/` change without a fragment or `changelog: none`. |
| `bump.sh <version>` | Set both surfaces, refresh `Cargo.lock`, assemble fragments, stamp the changelog, stage the result. |
| `extract-notes.sh <version>` | Print a `CHANGELOG.md` section body; fail when it is missing or empty. |
| `verify-approval.sh <version> <owner/repo>` | Fail a stable release without an operator approval comment. |
| `package.sh <version> <target>` | Pack a built binary into `dist/deslag-<version>-<target>.tar.gz`. |
| `checksums.sh <dir>` | Write `<dir>/SHA256SUMS` for every archive in `<dir>`. |

## 1. Collect changelog fragments

Each pull request with a user-visible change adds `changes/<slug>.md`. The
format and the `changelog: none` opt-out are in `changes/README.md`.

## 2. Bump

On a branch from an up-to-date `main`, run from the repository root:

```sh
scripts/release/bump.sh 0.1.0-alpha.1
git diff --cached
```

The script sets both version surfaces, refreshes `Cargo.lock` offline, folds
every fragment into `## Unreleased`, stamps that section as
`## 0.1.0-alpha.1 - <UTC date>` and adds a new empty `## Unreleased` above it.
It stages exactly those files and the fragment deletions. It never commits,
tags or pushes. It refuses an invalid SemVer version, a version that
`CHANGELOG.md` already has, an unknown section heading, and an empty release
(empty `## Unreleased` and no fragments). A refusal changes no file.

Edit the stamped section if the wording needs work. Then commit, open a pull
request and merge it to `main`.

## 3. Approve a stable release

A prerelease version (one with a `-` suffix, such as `0.1.0-alpha.1`) skips
this step.

A stable version needs an approval record. The repository owner posts an
issue comment, usually on the release epic, with a line that reads exactly:

```text
approve release v1.0.0
```

`verify-approval.sh` accepts the comment only when all of these hold:

- The author is the repository owner. Set the `RELEASE_OPERATOR` repository
  variable to name another account, for example when an organization owns
  the repository.
- One line of the body, trimmed and compared case-insensitively, equals the
  phrase. The phrase inside a longer sentence does not count.
- The comment was created after the committer date of the tagged commit. Post
  the approval after the bump commit lands on `main`.

Any lookup failure fails the release.

## 4. Tag

```sh
git switch main && git pull --ff-only
git tag -a v0.1.0-alpha.1 -m v0.1.0-alpha.1
git push origin v0.1.0-alpha.1
```

The workflow then runs three jobs:

1. `verify` fails unless the tagged commit is on `main`, the tag matches both
   version surfaces and, for a stable version, the approval exists. It extracts
   the release notes from the `## <version>` section of `CHANGELOG.md`.
2. `build` runs only after `verify` passes. It builds `deslag` with
   `cargo build --release --locked` for `aarch64-apple-darwin` (macOS 14) and
   `x86_64-unknown-linux-gnu` (Ubuntu), packs each into a `.tar.gz` and creates
   a build provenance attestation for it.
3. `release` writes `SHA256SUMS` and creates the GitHub Release with both
   archives, `SHA256SUMS` and the extracted notes. A version with a `-` suffix
   is marked as a prerelease.

A failed `verify` job stops the run before any build or release. Re-running a
tag run is safe: when the release already exists, the run replaces its assets
and keeps its notes.

## Dry run

Run the workflow manually from `main` (Actions, `release`, Run workflow, or
`gh workflow run release.yml --ref main`). A dry run checks the version
surfaces and the `main` ancestry, builds and packs both targets, writes
`SHA256SUMS` and prints the release notes. It extracts the notes from the
current version section, or from `## Unreleased` when that section does not
exist yet. A dry run skips the approval check, creates no attestation and
publishes nothing.

Locally, on macOS:

```sh
cargo build --release --locked --target aarch64-apple-darwin -p deslag-cli
scripts/release/package.sh "$(scripts/release/check-versions.sh)" aarch64-apple-darwin
scripts/release/checksums.sh dist
scripts/release/extract-notes.sh Unreleased
```

`dist/` is ignored by git.

## Verify a download

```sh
sha256sum -c SHA256SUMS          # macOS: shasum -a 256 -c SHA256SUMS
gh attestation verify deslag-0.1.0-alpha.1-aarch64-apple-darwin.tar.gz --repo mfittko/deslag
```

## Repository settings

- Protect `main` so that only reviewed pull requests change it. The release
  trusts every commit on `main`.
- Add a tag ruleset for `v*` that restricts tag creation to maintainers.
- Make the `ci` and `changelog` checks required for pull requests into `main`.
