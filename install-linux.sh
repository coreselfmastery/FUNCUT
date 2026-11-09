#!/usr/bin/env bash
# FUNCUT - the one-line Linux installer.
#
#   ./install-linux.sh                 AppImage into ~/.local/bin (no root)
#   ./install-linux.sh --deb           the .deb, through apt (asks for sudo)
#   ./install-linux.sh --rpm           the .rpm, through dnf (asks for sudo)
#   ./install-linux.sh --pacman        the Arch package, through pacman (sudo)
#   ./install-linux.sh --list          show what the releases hold, install nothing
#   ./install-linux.sh --repo o/r      a fork to prefer over this one
#   ./install-linux.sh --use-upstream  take Concat's own releases when the fork
#                                       has none; OWNER/NAME after it, for a fork of a fork
#
# What comes down beside the app. The default is everything a first run
# needs: the core models (whisper base.en for captions, Kokoro int8 for
# speech, the person-cutout and enhance networks), the sound library's
# manifest and the sticker shelf's index - all digest-checked, all into
# the app's own data folder, exactly where the editor's Settings page
# would put them, so the first run asks for nothing. Two flags tune it:
#
#   --no-models      install the app alone; the downloads stay where they
#                    always were, in Settings on first use
#   --all-models     every model in models/manifest.toml (~2.7 GB: every
#                    whisper size, both Kokoro builds, Pocket TTS, the
#                    whole Chatterbox bundle, both cutout networks and
#                    the SAM brushes)
#   --with-openvid   also the ~435 MB OpenVid stock index, so the stock
#                    shelf is ready the first time it is opened
#
# It looks for a published release on this repository first, and falls
# back to the upstream Concat releases when the fork has none - the two
# are the same program, built by the same workflows. A release built by
# the Release workflow publishes itself: CI passes on the tag first, and
# every bundle is launched where it was built before it uploads, so what
# this script finds is already public - nothing waits on a person to
# click Publish. (Drafts and pre-releases are still skipped below, the
# way they always were - a hand-made draft is none of this script's
# business until a person publishes it.)
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
# Which of the app's own downloads come down now, at install time,
# rather than on the first run that needs them: "core" (the default),
# "all" or "none".
MODELS="core"
# The OpenVid stock index (~435 MB) is the one piece big enough to
# stay opt-in: --with-openvid brings it too.
WITH_OPENVID="no"

say() { printf '%s\n' "$*"; }
die() { printf 'install: %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
    case "$1" in
        --appimage) KIND="appimage" ;;
        --deb) KIND="deb" ;;
        --rpm) KIND="rpm" ;;
        --pacman) KIND="pacman" ;;
        --list) LIST_ONLY="yes" ;;
        --no-models) MODELS="none" ;;
        --all-models) MODELS="all" ;;
        --models)
            [ $# -ge 2 ] || die "--models needs core, all or none after it"
            case "$2" in
                core|all|none) MODELS="$2" ;;
                *) die "--models wants core, all or none" ;;
            esac
            shift
            ;;
        --with-openvid) WITH_OPENVID="yes" ;;
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
            sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
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
# of a hundred and stops at the first short one: a full page may have
# another after it, a short one is the end. Python reads the JSON
# where there is Python, and grep carves out the download URLs where
# there is not - each URL names the release it belongs to, so the
# separators are cut out of them and the two paths print the same
# shape. A page that will not fetch or parse fails the whole read: a
# partial list that ends before the fork's own app releases would fall
# through to upstream, the same program but not this one.
release_assets() {
    local repo="$1" auth="${2:-}" page json page_out status count tmp_tags
    tmp_tags=$(mktemp) || return 1
    for page in 1 2 3 4 5; do
        json=$(fetch "https://api.github.com/repos/$repo/releases?per_page=100&page=$page" - $auth) ||
            { rm -f "$tmp_tags"; return 1; }
        if command -v python3 >/dev/null 2>&1; then
            # Exit 2 is a short page - the last one there is; exit 1 is
            # an answer that is not a list: a refusal or a broken
            # answer, not an empty shelf, and the whole read fails
            # rather than half-succeeding into the wrong fallback.
            status=0
            page_out=$(printf '%s' "$json" | python3 -c '
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
sys.exit(2 if len(releases) < 100 else 0)
') || status=$?
            if [ "$status" -eq 1 ]; then
                rm -f "$tmp_tags"
                return 1
            fi
            [ -n "$page_out" ] && printf '%s\n' "$page_out"
            [ "$status" -eq 2 ] && break
        else
            # Every asset URL carries its release's tag between
            # "download/" and the file name: a change of tag is a change
            # of release, and the separator the python path prints from
            # the JSON is printed here from that. Neither draft-ness nor
            # prerelease-ness is in a URL, so the flags are read from the
            # JSON in object order - GitHub's serializer emits each
            # release's tag_name before its draft and prerelease flags -
            # and a draft's or prerelease's tag is skipped whole. The
            # python path above skips drafts and prereleases; this path
            # used to skip only prereleases, so a draft (visible to a
            # GITHUB_TOKEN call) could be picked and installed.
            printf '%s' "$json" | grep -oE '"tag_name": *"[^"]+"|"draft": *(true|false)|"prerelease": *(true|false)' |
                awk '
                    match($0, /"tag_name": *"[^"]+"/) {
                        tag = substr($0, RSTART, RLENGTH)
                        gsub(/.*"tag_name": *"|"$/, "", tag)
                        order[++n] = tag
                        current = tag
                        next
                    }
                    /"draft": *true/ { hidden[current] = 1 }
                    /"prerelease": *true/ { hidden[current] = 1 }
                    END { for (i = 1; i <= n; i++) if (!hidden[order[i]]) print order[i] }
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
                ' || true
            # A page short of a hundred releases is the last page. The
            # count is of tag_name fields the page carried, drafts and
            # prereleases included - they hold a place in a page the way
            # the python path's len(releases) counts them.
            count=$(printf '%s' "$json" | grep -c '"tag_name"') || true
            [ "${count:-0}" -lt 100 ] && break
        fi
    done
    rm -f "$tmp_tags"
    return 0
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
            *.pkg.tar.zst)
                [ "$kind" = pacman ] || continue
                case "$name" in
                    *-linux-"$arch".pkg.tar.zst) found="$name"$'\t'"$url" ;;
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
# The sum and the asset are both assets of one release block, and
# GitHub's asset order is the order they were uploaded in - the sums
# file can sit before the asset it covers as easily as after it, so the
# scan reads the whole block and takes its SHA256SUMS whichever side of
# the asset it landed on - never one from a later release, printing
# nothing when the block ends without one.
checksums_url() {
    printf '%s\n' "$1" | awk -F'\t' -v asset="$2" '
        function flush() {
            if (found && sums != "") {
                print sums
                done = 1
            }
            exit
        }
        $1 == "RELEASE" {
            if (in_block && found)
                flush()
            in_block = 1
            found = 0
            sums = ""
            next
        }
        $1 == "SHA256SUMS" { sums = $2; next }
        $1 == asset { found = 1; next }
        END { if (!done && found && sums != "") print sums }
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
        # Into place by rename, never by writing over what may be running:
        # Linux refuses to write an open executable ("text file busy"),
        # but replaces it beside itself atomically - the running one
        # keeps its bytes until it ends, and the next start is the new
        # one.
        cp "$TMP/$ASSET_NAME" "$DEST.new"
        chmod +x "$DEST.new"
        mv -f "$DEST.new" "$DEST"
        # The fork's own name for the same program, for the muscle
        # memory this repository is named for.
        ln -sf concat "$BIN_DIR/funcut"
        # The start menu: an AppImage in ~/.local/bin is invisible to a
        # desktop until something writes the entry, so this does - a
        # user-level .desktop naming the installed path, and the icons it
        # names, in the sizes the desktop looks for. No root needed, and
        # a later package install (which writes /usr/share/applications)
        # simply outranks it.
        APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
        ICONS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons"
        mkdir -p "$APPS_DIR"
        cat > "$APPS_DIR/concat.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=FUNCUT
Comment=Video editor
Exec=$DEST %U
Icon=concat
Categories=AudioVideo;Video;
Terminal=false
StartupWMClass=concat
DESKTOP
        for SIZE in 16 32 64 128 256 512; do
            mkdir -p "$ICONS_DIR/hicolor/${SIZE}x${SIZE}/apps"
            # The repository this script installs from carries the marks;
            # the upstream it falls back to carries the same ones.
            fetch "https://raw.githubusercontent.com/$REPO/main/assets/icons/concat_logo_${SIZE}.png" \
                "$ICONS_DIR/hicolor/${SIZE}x${SIZE}/apps/concat.png" \
                || fetch "https://raw.githubusercontent.com/$FALLBACK/main/assets/icons/concat_logo_${SIZE}.png" \
                "$ICONS_DIR/hicolor/${SIZE}x${SIZE}/apps/concat.png" \
                || rm -f "$ICONS_DIR/hicolor/${SIZE}x${SIZE}/apps/concat.png"
        done
        command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS_DIR" >/dev/null 2>&1 || true
        say "installed: $DEST"
        say "start menu: $APPS_DIR/concat.desktop"
        say ""
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
    pacman)
        # The releases carry an Arch package; --list has shown it all
        # along, and the flag that installs it was missing.
        command -v sudo >/dev/null 2>&1 || die "installing an Arch package needs sudo, which is not here - try --appimage"
        command -v pacman >/dev/null 2>&1 || die "no pacman here - try --appimage"
        sudo pacman -U --noconfirm "$TMP/$ASSET_NAME"
        say "installed: /opt/concat/concat (run it as 'concat')"
        ;;
esac

# ── the first run's downloads, now ──────────────────────────────
# Everything the editor would otherwise ask for on the first run that
# needs it, fetched here so a fresh install opens ready: the core models
# (whisper base.en, Kokoro int8, the person cutout, the enhance network),
# the sound shelf's manifest and the sticker shelf's index - each into
# the app's own data folder, at the exact path and name the editor's
# Settings page would have written, and each checked against the digest
# models/manifest.toml pins before it is moved into place. A file that
# is already there, whole, is left alone: rerunning the installer never
# re-downloads what it already installed.
#
# --no-models skips all of this; --all-models takes every row of the
# manifest (~2.7 GB); --with-openvid adds the stock index (~435 MB).
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/app.concat.editor"

# One model row from the manifest, by id: prints "file sha256 bytes
# family" for the row, nothing when the id is not there.
manifest_row() {
    awk -v want="$1" '
        /^\[\[model\]\]/ { id = ""; file = ""; sha = ""; bytes = ""; family = "" }
        /^id = / { gsub(/"/, ""); id = $3 }
        /^family = / { gsub(/"/, ""); family = $3 }
        /^file = / { gsub(/"/, ""); file = $3 }
        /^sha256 = / { gsub(/"/, ""); sha = $3 }
        /^bytes = / { gsub(/"/, ""); bytes = $3 }
        id == want && file != "" && sha != "" && bytes != "" {
            print file, sha, bytes, family; found = 1; exit
        }
        END { exit !found }
    ' "$MANIFEST"
}

# Checks a downloaded file against a digest, the way the app itself
# does: a mismatch is deleted, never installed.
digest_ok() {
    # digest_ok FILE SHA256
    if command -v sha256sum >/dev/null 2>&1; then
        printf '%s  %s\n' "$2" "$1" | sha256sum -c - >/dev/null 2>&1
    elif command -v shasum >/dev/null 2>&1; then
        printf '%s  %s\n' "$2" "$1" | shasum -a 256 -c - >/dev/null 2>&1
    else
        # No tool to check with: the file is not installed unchecked.
        return 1
    fi
}

# Places one model row: download from the mirror, check, put it where
# the engine looks. Says what it did; never dies - a download that
# cannot happen today is a download the editor will offer again on the
# first run that needs it.
install_model() {
    # install_model ID
    MID="$1"
    row=$(manifest_row "$MID") || { say "  $MID: no such row in the manifest - left for the editor"; return 0; }
    set -- $row
    MFILE="$1"; MSHA="$2"; MBYTES="$3"; MFAMILY="$4"
    case "$MFAMILY" in
        whisper) MTARGET="$DATA_DIR/whisper-models/$MFILE" ;;
        cutout)  MTARGET="$DATA_DIR/cutout-models/$MFILE" ;;
        tts)
            # The archive unpacks to tts-models/<id>/, which is what
            # counts as downloaded once its .onnx is in place.
            MTARGET="$DATA_DIR/tts-models/${MID%%.tar.bz2}/model.onnx"
            ;;
        chatterbox)
            # Nine files that land under tts-models/chatterbox-turbo-q8/,
            # each under the name its graph expects (the engine's own
            # FILES table: tokenizer.json at the root, the graphs in
            # onnx/, the prefix dropped).
            MLOCAL="${MFILE#chatterbox-turbo-}"
            MTARGET="$DATA_DIR/tts-models/chatterbox-turbo-q8/$MLOCAL"
            ;;
        *) say "  $MID: unknown family $MFAMILY - left for the editor"; return 0 ;;
    esac
    [ -n "$MTARGET" ] || return 0
    if [ -e "$MTARGET" ]; then
        say "  $MID: already there"
        return 0
    fi
    MB_HUMAN=$(awk -v b="$MBYTES" 'BEGIN { printf "%.0f MB", b / 1048576 }')
    if ! fetch "https://github.com/$FORK_REPO/releases/download/models-v1/$MFILE" "$TMP/model-file"; then
        say "  $MID: could not download it - the editor will offer it again"
        return 0
    fi
    if ! digest_ok "$TMP/model-file" "$MSHA"; then
        rm -f "$TMP/model-file"
        say "  $MID: checksum FAILED - not installed; the editor will offer it again"
        return 0
    fi
    mkdir -p "$(dirname "$MTARGET")"
    case "$MFAMILY" in
        tts)
            # Unpack beside the target and move the id folder in whole,
            # the same two steps the app's own download takes, so a
            # half-unpacked bundle is never what the engine finds.
            rm -rf "$TMP/model-unpack"
            mkdir -p "$TMP/model-unpack"
            if ! tar -xjf "$TMP/model-file" -C "$TMP/model-unpack"; then
                say "  $MID: could not unpack it - the editor will offer it again"
                rm -rf "$TMP/model-unpack"; rm -f "$TMP/model-file"
                return 0
            fi
            rm -rf "$DATA_DIR/tts-models/${MID%%.tar.bz2}"
            mv "$TMP/model-unpack/${MID%%.tar.bz2}" "$DATA_DIR/tts-models/${MID%%.tar.bz2}" \
                || say "  $MID: could not place it - the editor will offer it again"
            ;;
        *)
            mv -f "$TMP/model-file" "$MTARGET"
            ;;
    esac
    rm -rf "$TMP/model-unpack" "$TMP/model-file"
    say "  $MID: installed ($MB_HUMAN)"
}

if [ "$MODELS" != none ] && [ "$LIST_ONLY" = no ]; then
    say ""
    say "the editor's own downloads, now (so the first run asks for nothing):"
    MANIFEST="$TMP/manifest.toml"
    if fetch "https://raw.githubusercontent.com/$FORK_REPO/main/models/manifest.toml" "$MANIFEST"; then
        # The core set: what a first session touches - captions, speech,
        # background removal, enhance - with the sizes a small machine
        # can hold. --all-models takes every row instead.
        if [ "$MODELS" = all ]; then
            MODEL_IDS=$(awk '/^\[\[model\]\]/ { infound = 1; next } infound && /^id = / { gsub(/"/, ""); print $3; infound = 0 }' "$MANIFEST")
        else
            MODEL_IDS="whisper-base.en kokoro-int8-multi-lang-v1_0 cutout-person enhance-upscale"
        fi
        for MODEL_ID in $MODEL_IDS; do
            install_model "$MODEL_ID"
        done
        # The sound shelf's manifest: the 530-effect CC0 library's index,
        # read by the Audio page and the director's sounds_search alike.
        mkdir -p "$DATA_DIR/sounds"
        if [ ! -e "$DATA_DIR/sounds/manifest.json" ]; then
            if fetch "https://raw.githubusercontent.com/$FORK_REPO/main/audio/manifest.json" "$DATA_DIR/sounds/manifest.json"; then
                say "  the sound shelf's manifest: installed (530 effects, fetched one at a time as used)"
            else
                rm -f "$DATA_DIR/sounds/manifest.json"
                say "  the sound shelf's manifest: could not fetch it - the editor will offer it again"
            fi
        else
            say "  the sound shelf's manifest: already there"
        fi
        # The sticker shelf's index: 1,285 Fluent Emoji, digest-checked
        # by the app on load, so the same check runs here first.
        mkdir -p "$DATA_DIR/magick"
        if [ ! -e "$DATA_DIR/magick/fluent_index.tsv" ]; then
            if fetch "https://github.com/$FORK_REPO/releases/download/fluent-emoji-index-v1/fluent_index.tsv" "$TMP/fluent_index.tsv" \
                && digest_ok "$TMP/fluent_index.tsv" "bd12802847f2ec64787a89c17e46a792abbaa9fee7152b9e943a50600545cc8a"; then
                mv -f "$TMP/fluent_index.tsv" "$DATA_DIR/magick/fluent_index.tsv"
                say "  the sticker shelf's index: installed (1,285 stickers, fetched one at a time as used)"
            else
                rm -f "$TMP/fluent_index.tsv"
                say "  the sticker shelf's index: could not fetch it - the editor will offer it again"
            fi
        else
            say "  the sticker shelf's index: already there"
        fi
    else
        say "  could not fetch the models manifest - the editor's Settings page"
        say "  offers every one of these on first use, unchanged"
    fi
    if [ "$WITH_OPENVID" = yes ]; then
        # The stock shelf's index: ~435 MB, the one download big enough
        # to stay behind its own flag. Digest-checked against the same
        # constant the app pins (openvid.rs's INDEX_SHA256).
        mkdir -p "$DATA_DIR/openvid"
        if [ -e "$DATA_DIR/openvid/openvid-index.sqlite" ]; then
            say "  the OpenVid stock index: already there"
        elif fetch "https://github.com/$FORK_REPO/releases/download/openvid-index-v1/openvid-index.sqlite" "$TMP/openvid-index.partial" \
            && digest_ok "$TMP/openvid-index.partial" "9d3e83f190bcab9906910fa98b901d74ec9944674a891797d50a731d28682c09"; then
            mv -f "$TMP/openvid-index.partial" "$DATA_DIR/openvid/openvid-index.sqlite"
            say "  the OpenVid stock index: installed (937,957 clips)"
        else
            rm -f "$TMP/openvid-index.partial"
            say "  the OpenVid stock index: could not fetch it - the stock page's own"
            say "  one-time download offers it again"
        fi
    fi
fi

# ── the motion sidecar ───────────────────────────────────────
# Geneva draws a markup comp: the browserless HTML/CSS renderer the
# motion layer bakes with, one pinned binary. The Linux .deb/.rpm/
# AppImage built by CI already bundle it beside the executable - /opt/
# concat/geneva for the packages, which the engine's own discovery finds
# next to the binary without it being on PATH - so those installs are
# covered here too and not re-downloaded. What is left is source and
# tarball installs, where a machine that never touches a comp need
# never download one. The editor runs without it (a comp with no
# markup bakes on the native engine), so every failure here is said
# and carried on, never fatal.
if ! command -v geneva >/dev/null 2>&1 \
    && [ ! -x /opt/concat/geneva ] \
    && [ ! -x "$HOME/.local/bin/geneva" ]; then
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

else
    say ""
    say "the motion sidecar: geneva is already installed - left as it is"
fi

# ── the motion fonts ──────────────────────────────────────
# The motion presets draw in three faces - Inter for text, Archivo for
# display, JetBrains Mono for code - all OFL, vendored in this
# repository under assets/fonts. Geneva resolves a family name through
# fontconfig, so a machine without them falls back to whatever is
# installed and the comps' metrics move: same comp, different look.
# From a checkout the files are copied; over curl|bash they are fetched
# from the repository raw, which carries the same bytes. Installed into
# the user's own fonts folder, no root needed - and never fatal.
say ""
FONTS_SRC="$(cd "$(dirname "$0")" 2>/dev/null && pwd)/assets/fonts"
FONTS_HOME="${XDG_DATA_HOME:-$HOME/.local/share}/fonts"
mkdir -p "$FONTS_HOME"
FONTS_COPIED=0
if [ -d "$FONTS_SRC" ]; then
    for FACE in "$FONTS_SRC"/*.ttf; do
        [ -f "$FACE" ] || continue
        cp "$FACE" "$FONTS_HOME/" && FONTS_COPIED=$((FONTS_COPIED + 1))
    done
else
    for FACE in Archivo-Variable.ttf Inter-Variable.ttf JetBrainsMono-Variable.ttf; do
        [ -e "$FONTS_HOME/$FACE" ] && { FONTS_COPIED=$((FONTS_COPIED + 1)); continue; }
        if fetch "https://raw.githubusercontent.com/$FORK_REPO/main/assets/fonts/$FACE" "$FONTS_HOME/$FACE"; then
            FONTS_COPIED=$((FONTS_COPIED + 1))
        else
            rm -f "$FONTS_HOME/$FACE"
        fi
    done
fi
if [ "$FONTS_COPIED" -gt 0 ]; then
    command -v fc-cache >/dev/null 2>&1 && fc-cache -f "$FONTS_HOME" >/dev/null 2>&1
    say "motion fonts installed: $FONTS_COPIED faces for the comps' own look"
else
    say "motion fonts: none installed - comps fall back to the machine's faces"
fi

say ""
say "Next, on a small machine:"
say "  - if the window will not open: install your distro's Vulkan drivers"
say "    (mesa-vulkan-drivers, or lavapipe for software rendering)"
say "  - give the machine breathing room: 4 GB of zram or swap"
say "  - the rest of the notes live in docs/FUNCUT.md"
