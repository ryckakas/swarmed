#!/usr/bin/env bash
#
# Builds what a release ships, into target/release-assets: Swarmed.app for Apple silicon and Intel
# in one, with the swarmed command inside it, as a zip (for install.sh and the Homebrew cask) and
# as a disk image (for downloading by hand), each with a .sha256 beside it; and as the archive the
# app updates itself from, with its signature (.app.tar.gz and .app.tar.gz.sig).
#
# Signing that archive needs the updater's private key in TAURI_SIGNING_PRIVATE_KEY, and its
# password, if it has one, in TAURI_SIGNING_PRIVATE_KEY_PASSWORD (RELEASING.md).
#
# release.yml runs it on a Mac runner. It runs the same way on a Mac with both targets installed:
#
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
#   pnpm install
#   scripts/build-release.sh

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${root}"

version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' Cargo.toml)"
if [ -z "${version}" ]; then
    echo "FAIL: no version under [workspace.package] in Cargo.toml" >&2
    exit 1
fi

out=target/release-assets
name="Swarmed-${version}-macos-universal"
binaries=crates/app/binaries

# The app carries the swarmed command (bundle.externalBin in tauri.release.conf.json), so one
# download holds both and they are always the same version. Tauri looks for it once per
# architecture while building the app for each, and joined for the universal bundle.
mkdir -p "${binaries}"
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release --locked -p swarmed --target "${target}"
    cp "target/${target}/release/swarmed" "${binaries}/swarmed-${target}"
done
lipo -create -output "${binaries}/swarmed-universal-apple-darwin" \
    "${binaries}/swarmed-aarch64-apple-darwin" "${binaries}/swarmed-x86_64-apple-darwin"

# An app without the updater's public key could never take an update, and nobody would know
# until the next release: refuse to build one.
if ! node -e 'process.exit(require("./crates/app/tauri.conf.json").plugins.updater.pubkey ? 0 : 1)'; then
    echo "FAIL: plugins.updater.pubkey in crates/app/tauri.conf.json is empty (RELEASING.md)" >&2
    exit 1
fi

# tauri.conf.json's beforeBuildCommand builds the UI first.
pnpm exec tauri build --target universal-apple-darwin \
    --config crates/app/tauri.release.conf.json -- --locked

bundle=target/universal-apple-darwin/release/bundle
rm -rf "${out}"
mkdir -p "${out}"
# ditto keeps the bundle as macOS expects it, signature included. Without the extended attributes
# (macOS marks every file it builds with one), resource forks and ACLs: ditto stores those as
# ._name files, which Homebrew's unzip unpacks into the bundle as files of their own, and a bundle
# with files its signature does not list is refused as damaged.
ditto -c -k --norsrc --noextattr --noacl --keepParent "${bundle}/macos/Swarmed.app" \
    "${out}/${name}.zip"
cp "${bundle}/dmg/Swarmed_${version}_universal.dmg" "${out}/${name}.dmg"
cp "${bundle}/macos/Swarmed.app.tar.gz" "${out}/${name}.app.tar.gz"
cp "${bundle}/macos/Swarmed.app.tar.gz.sig" "${out}/${name}.app.tar.gz.sig"
(
    cd "${out}"
    for file in "${name}.zip" "${name}.dmg"; do
        shasum -a 256 "${file}" >"${file}.sha256"
    done
)

echo "OK: ${out}/${name}.zip, .dmg and .app.tar.gz"
