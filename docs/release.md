# Release runbook

A release is a `v<version>` tag on a `main` commit. The tag push runs
`.github/workflows/release.yml`, which builds, attests and publishes the GitHub
Release. Nothing else publishes artifacts. crates.io, Homebrew and container
images are out of scope.

## Precondition: attestations

Build provenance attestations need a public repository, or a private
repository on a GitHub plan that supports attestations for private
repositories. Meet this precondition before the first tag. Otherwise the
`publish` job fails at its attestation step, before it creates the release, so
nothing is published.

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
| `check-changelog-fragment.sh <base>` | Fail a `crates/` or `plugins/` change without a valid fragment or `changelog: none`. |
| `lib.sh` | The fragment format rules, sourced by `bump.sh` and `check-changelog-fragment.sh`. |
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
`CHANGELOG.md` already has, a fragment or `## Unreleased` line that breaks the
fragment format in `changes/README.md`, and an empty release (empty
`## Unreleased` and no fragments). The refusals happen before the first write.
When a later step fails, for example `cargo update`, the script restores the
original files. A refusal or a failure therefore changes no file.

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

The workflow then runs four jobs:

1. `verify` fails unless the tagged commit is on `main`, the tag matches both
   version surfaces and, for a stable version, the approval exists. It extracts
   the release notes from the `## <version>` section of `CHANGELOG.md`.
2. `build` runs only after `verify` passes. It builds `deslag` with
   `cargo build --release --locked` for `aarch64-apple-darwin` (macOS 14) and
   `x86_64-unknown-linux-gnu` (Ubuntu 22.04), and packs each into a `.tar.gz`.
3. `checksums` writes `SHA256SUMS` for both archives.
4. `publish` creates a build provenance attestation for each archive, then
   creates the GitHub Release with both archives, `SHA256SUMS` and the
   extracted notes. A version with a `-` suffix is marked as a prerelease.
   It is the only job with write permissions, and it runs for a tag push only.

The Linux binary links against glibc. Ubuntu 22.04 sets the baseline at
glibc 2.35, so the binary runs on Linux distributions with glibc 2.35 or newer.

A failed `verify` job stops the run before any build or release. Re-running a
tag run is safe: when the release already exists, the run uploads only the
assets that the release does not have yet. It never replaces an existing asset
and keeps the notes as created.

## Dry run

Start a dry run from `main` (Actions, `release`, Run workflow, or
`gh workflow run release.yml --ref main`). A dry run started from a branch
whose head is not on `main` fails the ancestry check. Starting a workflow run
needs write access to the repository.

A dry run checks the version surfaces and the `main` ancestry, builds and packs
both targets, writes and prints `SHA256SUMS` and prints the release notes. It
extracts the notes from the current version section, or from `## Unreleased`
when that section does not exist yet. A dry run skips the approval check and
the `publish` job. Every job it runs has read-only permissions, so it creates
no attestation and publishes nothing.

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
