# Releasing

[← Back to README](README.md)

A release ships Swarmed.app, with the `swarmed` command inside it, from one tag:

| Where | What | Made by |
| --- | --- | --- |
| GitHub release | `Swarmed-X.Y.Z-macos-universal.zip` and `.dmg`, each with a `.sha256`; the CHANGELOG section as notes | `release.yml` |
| `install.sh` | the zip from the latest release, checked against its `.sha256` | read from `main` |
| `ryckakas/homebrew-tap` | `Casks/swarmed.rb`, from `packaging/homebrew/swarmed.rb` | `release.yml` |
| crates.io | `swarmed-core`, then `swarmed` (the command alone); `swarmed-app` is not published | `release.yml` |

## Every release

1. Each pull request with a change a user would notice adds its line under `## [Unreleased]` in
   `CHANGELOG.md`, under Added, Changed, Fixed or Removed.
2. Actions → **Release prep** → Run workflow, with the version (`0.2.0`, no `v`). It runs
   `scripts/release-prep.sh`, which turns the Unreleased entries into the `## [0.2.0]` section and
   sets the version in `Cargo.toml`, `Cargo.lock` and `package.json`, then opens the PR
   "chore: release v0.2.0".
3. Read the notes in that PR and merge it once CI passes. `release-tag.yml` tags the merge commit
   `v0.2.0`, and `release.yml` does the rest.

If a job of `release.yml` fails, rerun it from the run's page: what is published already is
skipped. Without the release workflows, `scripts/release-prep.sh 0.2.0` runs the same locally, and
pushing the tag by hand starts `release.yml`.

To try a build before releasing it: `scripts/build-release.sh` writes `target/release-assets`,
and `SWARMED_DOWNLOAD_URL=file://$PWD/target/release-assets SWARMED_VERSION=<version> sh
install.sh` installs from there.

## Once, before the first release

- **The repository is public.** The install script and the cask download from the GitHub
  release, and a private repository's releases need a sign-in.
- **Secrets** (Settings → Secrets and variables → Actions):
  - `RELEASE_PAT`: a fine-grained token on this repository with Contents and Pull requests read
    and write. GitHub starts no workflow for what the default token pushes, so the release PR
    would get no CI and the tag no release.
  - `HOMEBREW_TAP_TOKEN`: a fine-grained token on `ryckakas/homebrew-tap` with Contents read and
    write.
  - `CARGO_REGISTRY_TOKEN`: a crates.io API token with the `publish-new` and `publish-update`
    scopes. Only a token can create a crate crates.io has never seen.

## After the first release: trusted publishing

Once `swarmed-core` and `swarmed` exist on crates.io, add a trusted publisher to each (crate →
Settings → Trusted Publishing): owner `ryckakas`, repository `swarmed`, workflow `release.yml`, no
environment. `release.yml` then publishes with a token GitHub's identity buys for that run, and
uses `CARGO_REGISTRY_TOKEN` only for a crate crates.io has not seen. Delete that secret and revoke
the token on crates.io, and add it again only for a new crate.

## Signing

The app is signed ad hoc, not with an Apple Developer ID, and not notarised. Downloaded through a
browser or Homebrew, which marks it as from the internet, macOS stops it the first time it opens
until it is allowed under System Settings → Privacy & Security → Open Anyway; the cask says so
when installed. `install.sh` downloads with curl, which leaves no such mark. Signing and
notarising in `release.yml` needs a Developer ID certificate and an app-specific password as
secrets.
