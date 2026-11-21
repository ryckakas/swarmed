#!/usr/bin/env bash
#
# Builds what a release ships, into target/release-assets: Swarmed.app for Apple silicon and Intel
# in one, with the swarmed command inside it, as a zip (for install.sh and the Homebrew cask) and
# as a disk image (for downloading by hand), each with a .sha256 beside it.
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

# tauri.conf.json's beforeBuildCommand builds the UI first.
pnpm exec tauri build --target universal-apple-darwin \
    --config crates/app/tauri.release.conf.json -- --locked

bundle=target/universal-apple-darwin/release/bundle
rm -rf "${out}"
mkdir -p "${out}"
# ditto keeps the bundle as macOS expects it, signature included.
ditto -c -k --keepParent "${bundle}/macos/Swarmed.app" "${out}/${name}.zip"
cp "${bundle}/dmg/Swarmed_${version}_universal.dmg" "${out}/${name}.dmg"
(
    cd "${out}"
    for file in "${name}.zip" "${name}.dmg"; do
        shasum -a 256 "${file}" >"${file}.sha256"
    done
)

echo "OK: ${out}/${name}.zip and .dmg"
