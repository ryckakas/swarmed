#!/usr/bin/env bash
#
# Prepares release VERSION: the entries under ## [Unreleased] in CHANGELOG.md become the
# ## [VERSION] section, and the version in Cargo.toml, Cargo.lock and package.json becomes VERSION.
#
# It refuses a version that is not X.Y.Z, is not above the current one or is tagged already, and an
# empty Unreleased section: nothing to release usually means the wrong branch is checked out.
#
# .github/workflows/release-prep.yml runs it and opens the release PR. It runs the same way from a
# checkout, on macOS or Linux:
#
#   scripts/release-prep.sh 0.2.0

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${root}"
repo_url=https://github.com/ryckakas/swarmed

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

# Whether version $1 is above version $2.
is_above() {
    local a b c x y z
    IFS=. read -r a b c <<<"$1"
    IFS=. read -r x y z <<<"$2"
    ((a > x || (a == x && (b > y || (b == y && c > z)))))
}

# Rewrites FILE with awk, given the rest of the arguments: options, then the program.
rewrite() {
    local file="$1"
    shift
    awk "$@" "${file}" >"${file}.tmp"
    mv "${file}.tmp" "${file}"
}

[ $# -eq 1 ] || fail "usage: $0 <version>  (X.Y.Z, no leading v)"
version="$1"
[[ "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] ||
    fail "version '${version}' is not X.Y.Z (no leading v, no pre-release suffix)"
tag="v${version}"

if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
    fail "tag ${tag} already exists"
fi

current="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' Cargo.toml)"
[ -n "${current}" ] || fail "no version under [workspace.package] in Cargo.toml"
is_above "${version}" "${current}" || fail "${version} is not above the current version, ${current}"

grep -q '^## \[Unreleased\]$' CHANGELOG.md || fail "no '## [Unreleased]' heading in CHANGELOG.md"
grep -q '^\[Unreleased\]: ' CHANGELOG.md ||
    fail "no '[Unreleased]: ' link definition at the bottom of CHANGELOG.md"
if grep -q "^## \[${version}\]" CHANGELOG.md; then
    fail "CHANGELOG.md already has a [${version}] section"
fi
entries="$(awk '/^## \[Unreleased\]$/ { inside = 1; next } /^## \[|^\[[^]]+\]: / { inside = 0 }
    inside && NF' CHANGELOG.md | wc -l)"
[ "${entries}" -gt 0 ] || fail "the [Unreleased] section is empty: nothing to release"

# The newest release heading: the file runs from the newest release down.
previous="$(grep -oE '^## \[[0-9]+\.[0-9]+\.[0-9]+\]' CHANGELOG.md | head -1 | tr -d '#[] ' || true)"
if [ -n "${previous}" ]; then
    link="${repo_url}/compare/v${previous}...${tag}"
else
    link="${repo_url}/releases/tag/${tag}"
fi

# The programs below are awk's: $0 and the rest are awk's own, not the shell's.
# shellcheck disable=SC2016
rewrite CHANGELOG.md -v version="${version}" -v today="$(date -u +%Y-%m-%d)" \
    -v unreleased="${repo_url}/compare/${tag}...HEAD" -v link="${link}" '
    $0 == "## [Unreleased]" { print; print ""; print "## [" version "] - " today; next }
    /^\[Unreleased\]: / { print "[Unreleased]: " unreleased; print "[" version "]: " link; next }
    { print }
'

# The workspace's version, and the exact one `swarmed` requires of swarmed-core (`=X.Y.Z`): the
# core makes no promise about its API, so the command builds with its own and no other.
# shellcheck disable=SC2016
rewrite Cargo.toml -v old="\"${current}\"" -v new="\"${version}\"" \
    -v pinned="\"=${current}\"" -v repinned="\"=${version}\"" '
    /^\[/ { section = $0 }
    section == "[workspace.package]" && $0 == "version = " old { $0 = "version = " new }
    section == "[workspace.dependencies]" && index($0, "swarmed-core = ") == 1 {
        at = index($0, "version = " pinned)
        if (at) {
            rest = substr($0, at + length("version = " pinned))
            $0 = substr($0, 1, at - 1) "version = " repinned rest
        }
    }
    { print }
'

rewrite package.json -v new="${version}" '
    !done && /^    "version": "/ { sub(/"version": "[^"]*"/, "\"version\": \"" new "\""); done = 1 }
    { print }
'

# Only the workspace's own entries change in the lockfile.
cargo update --workspace --quiet

for file in Cargo.toml Cargo.lock package.json; do
    grep -q "\"${version}\"" "${file}" || fail "${file} does not name ${version}: check it by hand"
done
if grep -q "\"=\{0,1\}${current}\"" Cargo.toml; then
    fail "Cargo.toml still names ${current}: check it by hand"
fi
grep -q "^swarmed-core = .*version = \"=${version}\"" Cargo.toml ||
    fail "Cargo.toml does not require swarmed-core =${version}: check it by hand"

echo "OK: ${version} prepared (previous release: ${previous:-none})"
