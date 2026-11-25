#!/usr/bin/env bash
#
# Builds what a release ships, into target/release-assets: Swarmed.app for Apple silicon and Intel
# in one, with the swarmed command inside it, as a zip (for install.sh and the Homebrew cask) and
# as a disk image (for downloading by hand), each with a .sha256 beside it; and as the archive the
# app updates itself from, with its signature (.app.tar.gz and .app.tar.gz.sig).
#
# It has two steps, which release.yml runs on Mac runners of their own, so that the two
# architectures build at the same time:
#
#   scripts/build-release.sh build <target>   the command and the app for one architecture,
#                                             aarch64-apple-darwin or x86_64-apple-darwin, left
#                                             in target/<target>/release
#   scripts/build-release.sh bundle           both architectures joined into one app, bundled
#                                             and signed
#
# Named no step, it runs them all, one after another. It runs the same way on a Mac with both
# targets installed:
#
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
#   pnpm install
#   scripts/build-release.sh
#
# Signing the archive (bundle) needs the updater's private key in TAURI_SIGNING_PRIVATE_KEY, and
# its password, if it has one, in TAURI_SIGNING_PRIVATE_KEY_PASSWORD (RELEASING.md).

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${root}"

version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' Cargo.toml)"
if [ -z "${version}" ]; then
    echo "FAIL: no version under [workspace.package] in Cargo.toml" >&2
    exit 1
fi

targets=(aarch64-apple-darwin x86_64-apple-darwin)
config=crates/app/tauri.release.conf.json
binaries=crates/app/binaries

usage() {
    echo "usage: scripts/build-release.sh [build <target> | bundle]" >&2
    echo "       <target> is one of: ${targets[*]}" >&2
    exit 2
}

build() {
    local target="$1"
    case "${target}" in
    aarch64-apple-darwin | x86_64-apple-darwin) ;;
    *) usage ;;
    esac

    # An app without the updater's public key could never take an update, and nobody would know
    # until the next release: refuse to build one.
    if ! node -e 'process.exit(require("./crates/app/tauri.conf.json").plugins.updater.pubkey ? 0 : 1)'; then
        echo "FAIL: plugins.updater.pubkey in crates/app/tauri.conf.json is empty (RELEASING.md)" >&2
        exit 1
    fi

    # The app carries the swarmed command (bundle.externalBin in tauri.release.conf.json), so one
    # download holds both and they are always the same version. Tauri looks for it by the
    # target's name while building the app, and by universal-apple-darwin while bundling it.
    cargo build --release --locked -p swarmed --target "${target}"
    mkdir -p "${binaries}"
    cp "target/${target}/release/swarmed" "${binaries}/swarmed-${target}"

    # Only an app built here updates itself (from_sources in crates/app/src/update.rs): a release
    # must not take the place of what somebody built from the sources.
    export SWARMED_RELEASE=1

    # tauri.conf.json's beforeBuildCommand builds the UI first, which the app embeds.
    pnpm exec tauri build --target "${target}" --no-bundle --config "${config}" -- --locked
}

bundle() {
    local universal=target/universal-apple-darwin/release
    local target bin
    for target in "${targets[@]}"; do
        for bin in swarmed swarmed-app; do
            if [ ! -f "target/${target}/release/${bin}" ]; then
                echo "FAIL: no target/${target}/release/${bin}: run build ${target} first" >&2
                exit 1
            fi
        done
    done

    # The two builds joined, where tauri build --target universal-apple-darwin would leave them.
    # The mode is set again: a file passed between jobs as an artifact loses it.
    mkdir -p "${universal}" "${binaries}"
    lipo -create -output "${universal}/swarmed-app" \
        target/aarch64-apple-darwin/release/swarmed-app target/x86_64-apple-darwin/release/swarmed-app
    lipo -create -output "${binaries}/swarmed-universal-apple-darwin" \
        target/aarch64-apple-darwin/release/swarmed target/x86_64-apple-darwin/release/swarmed
    chmod 755 "${universal}/swarmed-app" "${binaries}/swarmed-universal-apple-darwin"

    pnpm exec tauri bundle --target universal-apple-darwin --config "${config}"

    local bundled=target/universal-apple-darwin/release/bundle
    local app="${bundled}/macos/Swarmed.app"
    # Each program in the app has to run, on both architectures. lipo checks one at a time.
    local arch
    for bin in swarmed swarmed-app; do
        for arch in arm64 x86_64; do
            if [ ! -x "${app}/Contents/MacOS/${bin}" ] ||
                ! lipo "${app}/Contents/MacOS/${bin}" -verify_arch "${arch}"; then
                echo "FAIL: ${app}/Contents/MacOS/${bin} is not an executable for ${arch}" >&2
                exit 1
            fi
        done
    done

    local out=target/release-assets
    local name="Swarmed-${version}-macos-universal"
    rm -rf "${out}"
    mkdir -p "${out}"
    # ditto keeps the bundle as macOS expects it, signature included. Without the extended
    # attributes (macOS marks every file it builds with one), resource forks and ACLs: ditto
    # stores those as ._name files, which Homebrew's unzip unpacks into the bundle as files of
    # their own, and a bundle with files its signature does not list is refused as damaged.
    ditto -c -k --norsrc --noextattr --noacl --keepParent "${app}" "${out}/${name}.zip"
    cp "${bundled}/dmg/Swarmed_${version}_universal.dmg" "${out}/${name}.dmg"
    cp "${bundled}/macos/Swarmed.app.tar.gz" "${out}/${name}.app.tar.gz"
    cp "${bundled}/macos/Swarmed.app.tar.gz.sig" "${out}/${name}.app.tar.gz.sig"

    # Signed with any other key than the one built into the app, the release would build and be
    # published, and every installed app would refuse it. A minisign key and a signature made
    # with it both carry the key's id: the eight bytes after the two that name the algorithm.
    if ! node -e '
        const id = (text) => {
            const line = Buffer.from(text, "base64").toString().split("\n")[1] ?? "";
            return Buffer.from(line, "base64").subarray(2, 10).toString("hex");
        };
        const built = id(require("./crates/app/tauri.conf.json").plugins.updater.pubkey);
        const signed = id(require("fs").readFileSync(process.argv[1], "utf8"));
        process.exit(built.length === 16 && built === signed ? 0 : 1);
    ' "${out}/${name}.app.tar.gz.sig"; then
        echo "FAIL: the archive is not signed with the key in plugins.updater.pubkey (RELEASING.md)" >&2
        exit 1
    fi
    (
        cd "${out}"
        for file in "${name}.zip" "${name}.dmg"; do
            shasum -a 256 "${file}" >"${file}.sha256"
        done
    )

    echo "OK: ${out}/${name}.zip, .dmg and .app.tar.gz"
}

case "${1:-}" in
build)
    [ "$#" -eq 2 ] || usage
    build "$2"
    ;;
bundle)
    [ "$#" -eq 1 ] || usage
    bundle
    ;;
"")
    for target in "${targets[@]}"; do
        build "${target}"
    done
    bundle
    ;;
*) usage ;;
esac
