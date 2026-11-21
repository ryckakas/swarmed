#!/bin/sh
# Installs Swarmed from its GitHub release: Swarmed.app, and the swarmed command it carries.
#
#   curl -fsSL https://raw.githubusercontent.com/ryckakas/swarmed/main/install.sh | sh
#
# Run again to update. Settings, all optional:
#
#   SWARMED_VERSION=0.1.0       that release instead of the latest
#   SWARMED_APP_DIR=<folder>    where Swarmed.app goes; /Applications when it can be written to,
#                               ~/Applications otherwise
#   SWARMED_BIN_DIR=<folder>    where the swarmed command is linked; ~/.local/bin by default
#   SWARMED_DOWNLOAD_URL=<url>  where the release files are, to try a build before releasing it
#
# Everything runs from main, called on the last line, so a download cut short runs nothing.

REPO=ryckakas/swarmed
APP=Swarmed.app

say() {
    printf 'swarmed: %s\n' "$1"
}

fail() {
    printf 'swarmed: %s\n' "$1" >&2
    exit 1
}

check_system() {
    [ "$(uname -s)" = Darwin ] || fail "Swarmed runs on macOS only"
    for tool in curl ditto shasum; do
        command -v "$tool" >/dev/null 2>&1 || fail "$tool is missing"
    done
}

# The latest release's version, from where GitHub's "latest" link leads: no API call, so no
# rate limit.
latest_version() {
    url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest") ||
        fail "couldn't find the latest release at https://github.com/$REPO/releases"
    case "$url" in
    */tag/v*) printf '%s\n' "${url##*/tag/v}" ;;
    *) fail "there is no release of Swarmed yet" ;;
    esac
}

app_folder() {
    if [ -n "${SWARMED_APP_DIR:-}" ]; then
        printf '%s\n' "$SWARMED_APP_DIR"
    elif [ -w /Applications ]; then
        printf '%s\n' /Applications
    else
        printf '%s\n' "$HOME/Applications"
    fi
}

# Downloads the archive and its checksum into $work, and refuses an archive that doesn't match.
download() {
    file="Swarmed-$version-macos-universal.zip"
    base="${SWARMED_DOWNLOAD_URL:-https://github.com/$REPO/releases/download/v$version}"
    say "downloading Swarmed $version"
    curl -fSL --progress-bar -o "$work/$file" "$base/$file" || fail "couldn't download $base/$file"
    curl -fsSL -o "$work/$file.sha256" "$base/$file.sha256" || fail "couldn't download its checksum"
    expected=$(cut -d ' ' -f 1 <"$work/$file.sha256")
    actual=$(shasum -a 256 "$work/$file" | cut -d ' ' -f 1)
    [ -n "$expected" ] && [ "$expected" = "$actual" ] ||
        fail "the download doesn't match its checksum; nothing was installed"
    ditto -x -k "$work/$file" "$work/unpacked" || fail "couldn't unpack $file"
    [ -x "$work/unpacked/$APP/Contents/MacOS/swarmed" ] || fail "$file holds no $APP"
}

check_not_running() {
    if pgrep -x swarmed-app >/dev/null 2>&1; then
        fail "Swarmed is running; quit it, then run this again"
    fi
}

# Replaces the app in one move, so a failure leaves the one that was there.
install_app() {
    mkdir -p "$apps" || fail "couldn't create $apps"
    if [ -e "$apps/$APP" ]; then
        mv "$apps/$APP" "$work/previous.app" || fail "couldn't replace $apps/$APP"
    fi
    if ! mv "$work/unpacked/$APP" "$apps/$APP"; then
        [ -e "$work/previous.app" ] && mv "$work/previous.app" "$apps/$APP"
        fail "couldn't move $APP into $apps"
    fi
}

link_command() {
    bin="${SWARMED_BIN_DIR:-$HOME/.local/bin}"
    mkdir -p "$bin" || fail "couldn't create $bin"
    ln -sf "$apps/$APP/Contents/MacOS/swarmed" "$bin/swarmed" || fail "couldn't link $bin/swarmed"
    case ":$PATH:" in
    *":$bin:"*) ;;
    *) say "add $bin to your PATH to use the swarmed command" ;;
    esac
}

main() {
    check_system
    check_not_running
    # Apart from the assignment: a failure inside $(...) ends only that subshell.
    if [ -n "${SWARMED_VERSION:-}" ]; then
        version="$SWARMED_VERSION"
    else
        version=$(latest_version) || exit 1
    fi
    version="${version#v}"
    apps=$(app_folder)
    work=$(mktemp -d) || fail "couldn't create a temporary folder"
    trap 'rm -rf "$work"' EXIT
    download
    install_app
    link_command
    say "installed Swarmed $version in $apps; open it with: open -a Swarmed"
}

main "$@" || exit 1
