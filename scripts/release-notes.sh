#!/usr/bin/env bash
#
# Prints the CHANGELOG.md section of release VERSION, without its heading. release.yml publishes
# it as the release's notes, and fails when it is missing or empty, so a release never goes out
# without them.
#
#   scripts/release-notes.sh 0.1.0

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [ $# -ne 1 ]; then
    echo "usage: $0 <version>" >&2
    exit 1
fi

# The section ends at the next release heading, or at the link definitions closing the file.
# Blank lines around it are left out.
notes="$(awk -v heading="## [$1]" '
    index($0, heading) == 1 { found = 1; next }
    found && (/^## \[/ || /^\[[^]]+\]: /) { exit }
    found { lines[++count] = $0; if (NF) { last = count; if (!first) first = count } }
    END { for (i = first; i <= last && first; i++) print lines[i] }
' "${root}/CHANGELOG.md")"

if [ -z "${notes}" ]; then
    echo "FAIL: CHANGELOG.md has no entries under ## [$1]" >&2
    exit 1
fi
printf '%s\n' "${notes}"
