#!/usr/bin/env bash
# FUNCUT - the one-line Linux installer.
#
#   ./install-linux.sh                 AppImage into ~/.local/bin (no root)
#   ./install-linux.sh --deb           the .deb, through apt (asks for sudo)
#   ./install-linux.sh --rpm           the .rpm, through dnf (asks for sudo)
#   ./install-linux.sh --list          show what the releases hold, install nothing
#   ./install-linux.sh --repo o/r      a fork to prefer over this one
#   ./install-linux.sh --use-upstream  take Concat's own releases when the fork
#                                       has none; OWNER/NAME after it, for a fork of a fork
#
# It looks for a published release on this repository first, and falls
# back to the upstream Concat releases when the fork has none - the two
# are the same program, built by the same workflows. A release built by
# the Release workflow starts as a *draft*: publish it from the
# repository's Releases page before this script (or the website, or
# anything but your own login) can see it.
#
# Needs curl or wget, and one of sha256sum or shasum to check the
# download. Nothing is installed until the check has passed.

set -euo pipefail

# ── where from ────────────────────────────────────────────────────────
# The repository this script came from, and the upstream it is a fork
# of. Override the first with --repo; the fallback is what --use-upstream
# reaches for, and it is also the default: a fork with no releases of its
# own should still end with the program installed.
FORK_REPO="coreselfmastery/FUNCUT"
UPSTREAM_REPO="jub0t/Concat"
REPO="$FORK_REPO"
FALLBACK="$UPSTREAM_REPO"

# ── what to install ───────────────────────────────────────────────────
KIND="appimage"
LIST_ONLY="no"

say() { printf '%s\n' "$*"; }
die() { printf 'install: %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
    case "$1" in
        --appimage) KIND="appimage" ;;
        --deb) KIND="deb" ;;
        --rpm) KIND="rpm" ;;
        --list) LIST_ONLY="yes" ;;
        --repo)
            [ $# -ge 2 ] || die "--repo needs OWNER/NAME after it"
            REPO="$2"
            shift
            ;;
        --use-upstream)
            # The documented form takes no value and names the upstream
            # this fork stands on; an OWNER/NAME after it overrides that
            # for a fork-of-a-fork.
            FALLBACK="$UPSTREAM_REPO"
            case "${2:-}" in
                [a-zA-Z0-9_-]*/[a-zA-Z0-9_.-]*) FALLBACK="$2"; shift ;;
            esac
            ;;
        -h|--help)
            sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) die "unknown option: $1 (try --help)" ;;
    esac
    shift
done

CURL=""
command -v curl >/dev/null 2>&1 && CURL="yes"

# fetch URL DEST - the file at URL into DEST, or to stdout when DEST is
# "-". Curl where there is curl, wget where there is not; either way,
# quiet on success, loud on failure, and three tries at the network.
fetch() {
    if [ "$2" = "-" ]; then
        if [ -n "$CURL" ]; then
            curl -fsSL --retry 3 -H 'Accept: application/vnd.github+json' "$1"
        else
            wget -qO- "$1"
        fi
    elif [ -n "$CURL" ]; then
        curl -fsSL --retry 3 -H 'Accept: application/vnd.github+json' -o "$2" "$1"
    else
        wget -q -O "$2" "$1"
    fi
}

[ -n "$CURL" ] || command -v wget >/dev/null 2>&1 ||
    die "needs curl or wget to download"

# The machine's architecture, as the release files name it: the two
# Linuxes the workflows build for.
case "$(uname -m)" in
    x86_64|amd64)   ARCH="x86_64" ;;
    aarch64|arm64)  ARCH="aarch64" ;;
    *) die "no Linux builds for $(uname -m); the releases hold x86_64 and aarch64" ;;
esac

# The published releases of `repo`, newest first, as a stream of
# tab-separated asset lines: NAME<TAB>URL, with a release separator
# line of just the tag. Drafts are invisible to anyone but their
# authors, so nothing here has to skip them. Python reads the JSON
# where there is Python, and grep carves out the download URLs where
# there is not - each URL names the release it belongs to, so the
# separators are cut out of them and the two paths print the same
# shape.
release_assets() {
    local repo="$1" page json
    for page in 1 2 3; do
        if command -v python3 >/dev/null 2>&1; then
            json=$(fetch "https://api.github.com/repos/$repo/releases?per_page=20&page=$page" -) || return 1
            printf '%s' "$json" | python3 -c '
import json, sys
try:
    releases = json.load(sys.stdin)
except Exception:
    sys.exit(1)
if isinstance(releases, dict):
    sys.exit(1)
for release in releases:
    if release.get("draft") or release.get("prerelease"):
        continue
    print("RELEASE\t" + release.get("tag_name", ""))
    for asset in release.get("assets", []):
        print(asset["name"] + "\t" + asset["browser_download_url"])
' && return 0
        else
            json=$(fetch "https://api.github.com/repos/$repo/releases?per_page=20&page=$page" -) || return 1
            # Every asset URL carries its release's tag between
            # "download/" and the file name: a change of tag is a change
            # of release, and the separator the python path prints from
            # the JSON is printed here from that.
            printf '%s' "$json" | grep -oE '"browser_download_url": *"[^"]+"' |
                sed -E 's/"browser_download_url": *"([^"]+)"/\1/' |
                awk -F/ -v OFS='\t' '{
                    tag = $(NF - 1)
                    if (tag != last) {
                        print "RELEASE", tag
                        last = tag
                    }
                    print $NF, $0
                }' && return 0
        fi
    done
    return 1
}

# The asset to install from a stream of the shape release_assets prints:
# the newest release that holds a file of the kind asked for this
# architecture. Linux package names carry the arch between "linux-"
# and the extension; the AppImage carries it before ".AppImage".
pick_asset() {
    local stream="$1" kind="$2" arch="$3" found="" name url
    while IFS=$'\t' read -r name url; do
        case "$name" in
            RELEASE) found="" ;;
            *.AppImage)
                [ "$kind" = appimage ] || continue
                case "$name" in
                    *-"$arch".AppImage) found="$name"$'\t'"$url" ;;
                esac
                ;;
            *.deb)
                [ "$kind" = deb ] || continue
                case "$name" in
                    *-linux-"$arch".deb) found="$name"$'\t'"$url" ;;
                esac
                ;;
            *.rpm)
                [ "$kind" = rpm ] || continue
                case "$name" in
                    *-linux-"$arch".rpm) found="$name"$'\t'"$url" ;;
                esac
                ;;
        esac
        # The first match in the newest release that has one is the one.
        [ -n "$found" ] && { printf '%s\n' "$found"; return 0; }
    done <<EOF
$stream
EOF
    return 1
}

# The checksums of the release an asset came from, when it has them.
# Inside one release the assets sort before SHA256SUMS, so the sum is
# only known to *follow* the asset: the scan remembers which block the
# asset was in, and takes the SHA256SUMS of that block - never one from
# a later release - printing nothing when the block ends without one.
checksums_url() {
    printf '%s\n' "$1" | awk -F'\t' -v asset="$2" '
        $1 == "RELEASE" {
            if (found) exit
            next
        }
        $1 == "SHA256SUMS" {
            if (found) { print $2; exit }
            next
        }
        $1 == asset { found = 1 }
    '
}

# ── find something to install ─────────────────────────────────────────
SEEN=""
VISITED=""
for repo in "$REPO" "$FALLBACK"; do
    [ -n "$repo" ] || continue
    [ "$repo" = "$VISITED" ] && continue
    VISITED="$repo"
    say "looking for a release on $repo ..."
    STREAM=$(release_assets "$repo") || { say "  none readable there"; continue; }
    [ -n "$STREAM" ] || { say "  no published releases"; continue; }

    if [ "$LIST_ONLY" = "yes" ]; then
        # Only what this script can install. The data releases beside the app
        # - the sound library's 430 files, the sticker index, the OpenVid
        # index - are shelves the app fetches for itself, so a release with
        # no package in it is not printed at all (and a release is named once,
        # not once per file). Listing stops at neither repo: a fork whose only
        # published releases are shelves still wants to show what upstream
        # can install, which is what the script itself would fall back to.
        LISTED=$(printf '%s\n' "$STREAM" | awk -F'\t' -v limit=3 '
            $1 == "RELEASE" {
                tag = $2
                order[++releases] = tag
                next
            }
            $1 ~ /\.(AppImage|deb|rpm|zst)$/ {
                held[tag]++
                asset[tag, held[tag]] = $1
            }
            END {
                shown = 0
                older = 0
                for (i = 1; i <= releases; i++) {
                    t = order[i]
                    if (!held[t]) continue   # a shelf, not a package release
                    if (shown >= limit) { older++; continue }
                    if (shown) print ""
                    print "  " t
                    # The API hands the assets back in no particular order;
                    # a list is read in one.
                    for (j = 2; j <= held[t]; j++) {
                        v = asset[t, j]
                        for (k = j - 1; k >= 1 && asset[t, k] > v; k--)
                            asset[t, k + 1] = asset[t, k]
                        asset[t, k + 1] = v
                    }
                    for (j = 1; j <= held[t]; j++) print "    " asset[t, j]
                    shown++
                }
                if (older)
                    print "  (and " older (older == 1 ? " older release" : " older releases") ")"
            }
        ')
        if [ -n "$LISTED" ]; then
            printf '%s\n' "$LISTED"
            SEEN="yes"
        else
            say "  no packages in its releases"
        fi
        continue
    fi

    if PICKED=$(pick_asset "$STREAM" "$KIND" "$ARCH"); then
        ASSET_NAME=${PICKED%%$'\t'*}
        ASSET_URL=${PICKED#*$'\t'}
        FROM_REPO="$repo"
        break
    fi
    say "  no $KIND for $ARCH in its releases"
done

if [ "$LIST_ONLY" = "yes" ]; then
    [ -n "$SEEN" ] || say "nothing to install: no $KIND for $ARCH in $REPO or $FALLBACK"
    exit 0
fi

[ -n "${ASSET_URL:-}" ] || die "nothing to install: no $KIND for $ARCH in $REPO or $FALLBACK.
If this fork has no published release yet, see docs/FUNCUT.md - one push
of a version tag builds one."

say "found: $ASSET_NAME (from $FROM_REPO)"

# ── download, check, install ──────────────────────────────────────────
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
say "downloading ..."
fetch "$ASSET_URL" "$TMP/$ASSET_NAME"

# The checksum, when the release carries one: the download is only
# installed once it has passed. A missing SHA256SUMS is said and carried
# on - an unsigned fork release is exactly what the fallback installs.
CHECK=$(checksums_url "$STREAM" "$ASSET_NAME")
if [ -n "$CHECK" ]; then
    if fetch "$CHECK" "$TMP/SHA256SUMS"; then
        if command -v sha256sum >/dev/null 2>&1; then
            (cd "$TMP" && grep -F "$ASSET_NAME" SHA256SUMS | sha256sum -c -)
            say "checksum ok"
        elif command -v shasum >/dev/null 2>&1; then
            (cd "$TMP" && grep -F "$ASSET_NAME" SHA256SUMS | shasum -a 256 -c -)
            say "checksum ok"
        else
            say "no sha256 tool found - installing unchecked"
        fi
    fi
else
    say "release carries no SHA256SUMS - installing unchecked"
fi

case "$KIND" in
    appimage)
        BIN_DIR="$HOME/.local/bin"
        mkdir -p "$BIN_DIR"
        # A stable name, whatever the version in the file is: this is
        # what a person types, and what the desktop entry's Exec says.
        DEST="$BIN_DIR/concat"
        cp "$TMP/$ASSET_NAME" "$DEST"
        chmod +x "$DEST"
        # The fork's own name for the same program, for the muscle
        # memory this repository is named for.
        ln -sf concat "$BIN_DIR/funcut"
        say ""
        say "installed: $DEST"
        case ":$PATH:" in
            *":$BIN_DIR:"*) ;;
            *) say "note: $BIN_DIR is not on your PATH - add it to use 'concat' from a terminal" ;;
        esac
        ;;
    deb)
        command -v sudo >/dev/null 2>&1 || die "installing a .deb needs sudo, which is not here - try --appimage"
        if command -v apt >/dev/null 2>&1; then
            sudo apt install -y "$TMP/$ASSET_NAME"
        elif command -v dpkg >/dev/null 2>&1; then
            sudo dpkg -i "$TMP/$ASSET_NAME" || sudo apt-get -f install -y
        else
            die "no apt or dpkg - try --appimage"
        fi
        say "installed: /opt/concat/concat (run it as 'concat')"
        ;;
    rpm)
        command -v sudo >/dev/null 2>&1 || die "installing an .rpm needs sudo, which is not here - try --appimage"
        if command -v dnf >/dev/null 2>&1; then
            sudo dnf install -y "$TMP/$ASSET_NAME"
        elif command -v rpm >/dev/null 2>&1; then
            sudo rpm -U --force "$TMP/$ASSET_NAME"
        else
            die "no dnf or rpm - try --appimage"
        fi
        say "installed: /opt/concat/concat (run it as 'concat')"
        ;;
esac

say ""
say "Next, on a small machine:"
say "  - if the window will not open: install your distro's Vulkan drivers"
say "    (mesa-vulkan-drivers, or lavapipe for software rendering)"
say "  - give the machine breathing room: 4 GB of zram or swap"
say "  - the rest of the notes live in docs/FUNCUT.md"
