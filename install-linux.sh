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
# Beside the app it offers the motion sidecar - Geneva v1.6.0, pinned by
# checksum - into ~/.local/bin: the editor runs without it (a comp with
# no markup bakes on the native engine), so its failure is said and
# never fatal.
#
# A repository can be private (a fork only its owner can see): the
# anonymous API is refused there, which reads to this script as "no
# releases" - and it would fall back to upstream, the same program but
# not this one. GITHUB_TOKEN (or GH_TOKEN, the gh CLI's own name for it)
# in the environment fixes that: a fine-grained token with read access
# to the repository is enough, and it is sent only to the API listings
# and the downloads of the repositories named here - never to a
# third-party download such as the sidecar's.
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

# A token for GitHub, when the repository this installs from is private:
# the anonymous API answers 403/404 for it, which this script would
# otherwise read as "no releases" and fall back to upstream. GITHUB_TOKEN
# or GH_TOKEN; sent only where the calls below say `auth` - the release
# listings and the downloads of the repositories named here, never a
# third-party URL. A fine-grained token with read access to the
# repository is enough.
GH_TOKEN_VALUE="${GITHUB_TOKEN:-${GH_TOKEN:-}}"

# The token never rides the command line: argv is world-readable through
# /proc and `ps`, and a header on argv would leak it for as long as the
# release listing and the ~100 MB download run. It travels in a config
# file instead - 600, made once up front, removed by the exit trap -
# which curl reads with -K and wget through WGETRC. It is made before
# the first fetch because `fetch` is called inside command substitutions
# (the release listings), where nothing it set would survive.
AUTH_CONF=""
TMP=""
cleanup() {
    [ -n "$TMP" ] && rm -rf "$TMP"
    [ -n "$AUTH_CONF" ] && rm -f "$AUTH_CONF"
    return 0
}
trap cleanup EXIT

if [ -n "$GH_TOKEN_VALUE" ]; then
    AUTH_CONF=$(mktemp "${TMPDIR:-/tmp}/concat-install.XXXXXX") ||
        die "could not make a private scratch file for the token"
    chmod 600 "$AUTH_CONF"
    {
        printf 'header = "Authorization: Bearer %s"\n' "$GH_TOKEN_VALUE"
        printf 'header = "Accept: application/vnd.github+json"\n'
    } >"$AUTH_CONF"
fi

# fetch URL DEST [auth] - the file at URL into DEST, or to stdout when
# DEST is "-". Curl where there is curl, wget where there is not; either
# way, quiet on success, loud on failure, and three tries at the network.
# The third word, `auth`, sends the token above with this one request -
# through the config file, never through argv.
fetch() {
    local conf=""
    if [ "${3:-}" = "auth" ] && [ -n "$AUTH_CONF" ]; then
        conf="$AUTH_CONF"
    fi
    if [ "$2" = "-" ]; then
        if [ -n "$CURL" ]; then
            if [ -n "$conf" ]; then
                curl -fsSL --retry 3 -K "$conf" "$1"
            else
                curl -fsSL --retry 3 -H 'Accept: application/vnd.github+json' "$1"
            fi
        elif [ -n "$conf" ]; then
            WGETRC="$conf" wget -qO- "$1"
        else
            wget -qO- "$1"
        fi
    elif [ -n "$CURL" ]; then
        if [ -n "$conf" ]; then
            curl -fsSL --retry 3 -K "$conf" -o "$2" "$1"
        else
            curl -fsSL --retry 3 -H 'Accept: application/vnd.github+json' -o "$2" "$1"
        fi
    elif [ -n "$conf" ]; then
        WGETRC="$conf" wget -q -O "$2" "$1"
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
# authors, so nothing here has to skip them; prereleases are not
# install material either and are skipped on both paths. Data-release
# tags (sound shelves, sticker and OpenVid indexes, the models mirror)
# can push the app releases pages deep, so this reads up to five pages
# of a hundred. Python reads the JSON where there is Python, and grep
# carves out the download URLs where there is not - each URL names the
# release it belongs to, so the separators are cut out of them and the
# two paths print the same shape.
release_assets() {
    local repo="$1" auth="${2:-}" page json tmp_tags
    tmp_tags=$(mktemp) || return 1
    for page in 1 2 3 4 5; do
        if command -v python3 >/dev/null 2>&1; then
            json=$(fetch "https://api.github.com/repos/$repo/releases?per_page=100&page=$page" - $auth) || return 1
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
            json=$(fetch "https://api.github.com/repos/$repo/releases?per_page=100&page=$page" - $auth) || return 1
            # Every asset URL carries its release's tag between
            # "download/" and the file name: a change of tag is a change
            # of release, and the separator the python path prints from
            # the JSON is printed here from that. Prerelease-ness is not
            # in a URL, so the flags are read from the JSON in object
            # order - GitHub's serializer emits one tag_name and one
            # prerelease flag per release object, in the same order - and
            # a prerelease's tag is skipped whole.
            printf '%s' "$json" | grep -oE '"tag_name": *"[^"]+"|"prerelease": *(true|false)' |
                paste - - |
                awk -F'\t' '
                    match($1, /"tag_name": *"[^"]+"/) {
                        tag = substr($1, RSTART, RLENGTH)
                        gsub(/.*"tag_name": *"|"$/, "", tag)
                        if ($2 ~ /true/) prerelease[tag] = 1
                        order[++n] = tag
                    }
                    END { for (i = 1; i <= n; i++) if (!prerelease[order[i]]) print order[i] }
                ' > "$tmp_tags" || true
            printf '%s' "$json" | grep -oE '"browser_download_url": *"[^"]+"' |
                sed -E 's/"browser_download_url": *"([^"]+)"/\1/' |
                awk -F/ -v OFS='\t' -v skipped="$tmp_tags" '
                    BEGIN {
                        while ((getline line < skipped) > 0) skip[line] = 1
                        close(skipped)
                    }
                    {
                        tag = $(NF - 1)
                        if (skip[tag]) next
                        if (tag != last) {
                            print "RELEASE", tag
                            last = tag
                        }
                        print $NF, $0
                    }
                ' && { rm -f "$tmp_tags"; return 0; }
        fi
    done
    rm -f "$tmp_tags"
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
    # The primary repository is asked with the token when there is one:
    # it is the one that can be private, and a token that cannot read it
    # is a mistake to stop for, not to fall back from - the fallback is
    # the same program, but not this one. Without a token the fall-back
    # stands: a public fork needs none, and a private one is said below.
    STREAM=$(release_assets "$repo" auth) || {
        if [ "$repo" = "$REPO" ] && [ -n "$GH_TOKEN_VALUE" ]; then
            die "the releases of $repo could not be read with the token given -
it may be refused (check its access and expiry), or the network is down"
        fi
        if [ "$repo" = "$REPO" ]; then
            say "  its releases could not be read: the anonymous API is
  refused there - a private repository, or a rate-limited network. A
  token fixes both: export GITHUB_TOKEN first
  (a fine-grained token with read access to this repository is enough),
  then run this installer again - the token is never passed on the
  command line, where ps and shell history would read it"
        else
            say "  none readable there"
        fi
        continue
    }
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
say "downloading ..."
# The token rides this download when the file belongs to the repository
# it was found on - a private one refuses the anonymous download too -
# and never does when it does not: a third party's URL must not carry it.
ASSET_AUTH=""
case "$ASSET_URL" in
    "https://github.com/$FROM_REPO/releases/download/"*) ASSET_AUTH="auth" ;;
esac
fetch "$ASSET_URL" "$TMP/$ASSET_NAME" $ASSET_AUTH

# The checksum. A release that carries a SHA256SUMS is a release that
# promises its bytes, and the promise is enforced: a sums file that
# cannot be fetched, an asset the sums do not name, or a machine with
# no tool to check them is a failed install, not an unchecked one - the
# download is a whole application, run by this user, from the network.
# Only a release with no SHA256SUMS at all installs with a notice.
CHECK=$(checksums_url "$STREAM" "$ASSET_NAME")
if [ -n "$CHECK" ]; then
    CHECK_AUTH=""
    case "$CHECK" in
        "https://github.com/$FROM_REPO/releases/download/"*) CHECK_AUTH="auth" ;;
    esac
    fetch "$CHECK" "$TMP/SHA256SUMS" $CHECK_AUTH \
        || die "could not fetch the release's SHA256SUMS - not installing unchecked"
    # The exact file, by the whole second field: a substring grep could
    # match a longer name that merely ends in this one.
    awk -v n="$ASSET_NAME" '$2 == n' "$TMP/SHA256SUMS" > "$TMP/MY_SUM"
    [ -s "$TMP/MY_SUM" ] \
        || die "SHA256SUMS names no $ASSET_NAME - not installing unchecked"
    if command -v sha256sum >/dev/null 2>&1; then
        (cd "$TMP" && sha256sum -c MY_SUM) \
            || die "checksum FAILED for $ASSET_NAME - not installing it"
        say "checksum ok"
    elif command -v shasum >/dev/null 2>&1; then
        (cd "$TMP" && shasum -a 256 -c MY_SUM) \
            || die "checksum FAILED for $ASSET_NAME - not installing it"
        say "checksum ok"
    else
        die "no sha256 tool found and the release carries a SHA256SUMS - not installing unchecked"
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

# ── the motion sidecar ───────────────────────────────────────
# Geneva draws a markup comp: the browserless HTML/CSS renderer the
# motion layer bakes with, one pinned binary. The Linux .deb/.rpm/
# AppImage built by CI already bundle it beside the executable; this
# branch covers source and tarball installs, where a machine that never
# touches a comp need never download one. The editor runs without it
# (a comp with no markup bakes on the native engine), so every failure
# here is said and carried on, never fatal.
if ! command -v geneva >/dev/null 2>&1; then
    say ""
    case "$ARCH" in
        x86_64)
            GENEVA_URL="https://github.com/geneva-render/geneva/releases/download/v1.6.0/geneva-v1.6.0-x86_64-unknown-linux-gnu.tar.gz"
            GENEVA_SHA256="c2c9d3b2f82621ba6e7cfd36b36a8a3df9d9ba2b7374d06245248e2fb4120e16"
            ;;
        aarch64)
            GENEVA_URL="https://github.com/geneva-render/geneva/releases/download/v1.6.0/geneva-v1.6.0-aarch64-unknown-linux-gnu.tar.gz"
            GENEVA_SHA256="8066d4ff388b5f2557678e168bca1e70c49d9624ee61cedad2bfcf0d008f1243"
            ;;
    esac
    say "the motion sidecar: geneva v1.6.0 (~16 MB, checksum-checked)"
    if fetch "$GENEVA_URL" "$TMP/geneva.tar.gz"; then
        GENEVA_CHECKED="no"
        if command -v sha256sum >/dev/null 2>&1; then
            if printf '%s  %s\n' "$GENEVA_SHA256" "$TMP/geneva.tar.gz" | sha256sum -c - >/dev/null 2>&1; then
                GENEVA_CHECKED="yes"
            fi
        elif command -v shasum >/dev/null 2>&1; then
            if printf '%s  %s\n' "$GENEVA_SHA256" "$TMP/geneva.tar.gz" | shasum -a 256 -c - >/dev/null 2>&1; then
                GENEVA_CHECKED="yes"
            fi
        else
            GENEVA_CHECKED="unchecked"
        fi
        case "$GENEVA_CHECKED" in
            yes|unchecked)
                [ "$GENEVA_CHECKED" = yes ] && say "  checksum ok" || say "  no sha256 tool found - installing unchecked"
                mkdir -p "$TMP/geneva-out" "$HOME/.local/bin"
                tar -xzf "$TMP/geneva.tar.gz" -C "$TMP/geneva-out" ||
                    { say "  could not unpack it - the editor runs without it"; }
                GENEVA_BIN=$(find "$TMP/geneva-out" -type f -name geneva 2>/dev/null | head -n 1 || true)
                if [ -n "$GENEVA_BIN" ]; then
                    cp "$GENEVA_BIN" "$HOME/.local/bin/geneva"
                    chmod +x "$HOME/.local/bin/geneva"
                    GENEVA_VERSION=$("$HOME/.local/bin/geneva" --version 2>/dev/null || true)
                    if [ -n "$GENEVA_VERSION" ]; then
                        say "  installed: $HOME/.local/bin/geneva ($GENEVA_VERSION)"
                    else
                        say "  installed but it did not run here - the editor runs without it"
                    fi
                else
                    say "  the tarball held no binary - the editor runs without it"
                fi
                ;;
            *)
                say "  checksum mismatch - not installed; the editor runs without it"
                ;;
        esac
    else
        say "  could not download it - the editor runs without it"
    fi

    # The motion presets draw in three faces - Inter for text, Archivo for
    # display, JetBrains Mono for code - all OFL, vendored in this
    # repository under assets/fonts. Geneva resolves a family name through
    # fontconfig, so a machine without them falls back to whatever is
    # installed and the comps' metrics move: same comp, different look.
    # Installed into the user's own fonts folder, no root needed.
    FONTS_SRC="$(cd "$(dirname "$0")" && pwd)/assets/fonts"
    if [ -d "$FONTS_SRC" ]; then
        mkdir -p "$HOME/.local/share/fonts"
        FONTS_COPIED=0
        for FACE in "$FONTS_SRC"/*.ttf; do
            [ -f "$FACE" ] || continue
            cp "$FACE" "$HOME/.local/share/fonts/" && FONTS_COPIED=$((FONTS_COPIED + 1))
        done
        if [ "$FONTS_COPIED" -gt 0 ]; then
            command -v fc-cache >/dev/null 2>&1 && fc-cache -f "$HOME/.local/share/fonts" >/dev/null 2>&1
            say "  motion fonts installed: $FONTS_COPIED faces for the comps' own look"
        fi
    fi
else
    say ""
    say "the motion sidecar: geneva is already on PATH - left as it is"
fi

say ""
say "Next, on a small machine:"
say "  - if the window will not open: install your distro's Vulkan drivers"
say "    (mesa-vulkan-drivers, or lavapipe for software rendering)"
say "  - give the machine breathing room: 4 GB of zram or swap"
say "  - the rest of the notes live in docs/FUNCUT.md"
