#!/bin/sh
# Konstruktor installer.
#
#   curl -fsSL https://raw.githubusercontent.com/arkitektio/konstruktor/master/install.sh | sh
#
# Downloads the binary for this machine, verifies it against the release's published
# checksums, installs it, and — when there is a terminal to talk to — goes straight into
# creating a hub in ~/MyHubs/<identifier>.
#
# Options (pass after `| sh -s --`):
#   --no-run            install only; do not start the wizard
#   --hub-dir <path>    put the hub here instead of ~/MyHubs/<identifier>
#   --version <tag>     a specific release, e.g. konstruktor-v0.0.1
#   --dir <path>        where to install (default: ~/.local/bin)

set -eu

REPO="arkitektio/konstruktor"
BIN_NAME="konstruktor"
INSTALL_DIR="${KONSTRUKTOR_INSTALL_DIR:-$HOME/.local/bin}"
VERSION=""
RUN_AFTER=1
HUB_PARENT="${KONSTRUKTOR_HUB_PARENT:-$HOME/MyHubs}"
HUB_DIR=""

die() {
    printf '\n  error: %s\n\n' "$1" >&2
    exit 1
}

say() { printf '  %s\n' "$1" >&2; }

while [ $# -gt 0 ]; do
    case "$1" in
        --no-run) RUN_AFTER=0 ;;
        --hub-dir) HUB_DIR="${2:-}"; [ -n "$HUB_DIR" ] || die "--hub-dir needs a path"; shift ;;
        --version) VERSION="${2:-}"; [ -n "$VERSION" ] || die "--version needs a tag"; shift ;;
        --dir) INSTALL_DIR="${2:-}"; [ -n "$INSTALL_DIR" ] || die "--dir needs a path"; shift ;;
        -h|--help)
            sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) die "unknown option: $1" ;;
    esac
    shift
done

# --- what are we running on ------------------------------------------------------------
detect_target() {
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)  os_part="unknown-linux-gnu" ;;
        Darwin) os_part="apple-darwin" ;;
        *) die "unsupported operating system: $os. Windows users: download the binary from
    https://github.com/$REPO/releases" ;;
    esac

    case "$arch" in
        x86_64|amd64)  arch_part="x86_64" ;;
        aarch64|arm64) arch_part="aarch64" ;;
        *) die "unsupported architecture: $arch" ;;
    esac

    printf '%s-%s' "$arch_part" "$os_part"
}

fetch() {
    # -f so a 404 is a failure rather than an HTML page written to the output file.
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$2" "$1"
    else
        die "neither curl nor wget is available"
    fi
}

TARGET="$(detect_target)"

if [ -n "$VERSION" ]; then
    BASE="https://github.com/$REPO/releases/download/$VERSION"
else
    BASE="https://github.com/$REPO/releases/latest/download"
fi

ASSET="$BIN_NAME-$TARGET"

printf '\n'
say "konstruktor · $TARGET"

TMP="$(mktemp -d)"
# shellcheck disable=SC2064
trap "rm -rf '$TMP'" EXIT INT TERM

say "Downloading…"
fetch "$BASE/$ASSET" "$TMP/$BIN_NAME" \
    || die "no build for $TARGET in that release.
    See https://github.com/$REPO/releases"

# --- verify -----------------------------------------------------------------------------
# A binary that installs itself into someone's PATH has to be worth trusting; a missing
# checksum file is a reason to stop, not to shrug.
if fetch "$BASE/SHA256SUMS" "$TMP/SHA256SUMS" 2>/dev/null; then
    expected="$(grep " $ASSET\$" "$TMP/SHA256SUMS" | awk '{print $1}' || true)"
    if [ -z "$expected" ]; then
        die "$ASSET is not listed in that release's SHA256SUMS"
    fi

    if command -v sha256sum >/dev/null 2>&1; then
        actual="$(sha256sum "$TMP/$BIN_NAME" | awk '{print $1}')"
    elif command -v shasum >/dev/null 2>&1; then
        actual="$(shasum -a 256 "$TMP/$BIN_NAME" | awk '{print $1}')"
    else
        actual=""
        say "! No sha256 tool found; skipping verification."
    fi

    if [ -n "$actual" ] && [ "$actual" != "$expected" ]; then
        die "checksum mismatch — refusing to install.
    expected $expected
    got      $actual"
    fi
    [ -n "$actual" ] && say "Checksum verified."
else
    die "that release publishes no SHA256SUMS — refusing to install unverified."
fi

# --- install ----------------------------------------------------------------------------
mkdir -p "$INSTALL_DIR"
chmod +x "$TMP/$BIN_NAME"
mv "$TMP/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"

BIN="$INSTALL_DIR/$BIN_NAME"
say "Installed to $BIN"

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        printf '\n'
        say "! $INSTALL_DIR is not on your PATH. Add it with:"
        say "    export PATH=\"\$PATH:$INSTALL_DIR\""
        ;;
esac

# --- and then actually make a hub --------------------------------------------------------
#
# `hub create` builds the hub in the current directory unless it is given one, and the
# current directory here is wherever the one-liner was pasted. So the installer names the
# folder rather than inheriting it: ~/MyHubs, one folder per hub, named after the hub
# itself, or whatever `--hub-dir` says.
#
# Which means the identifier has to be asked for out here, before there is a folder to
# name — it is passed on to the wizard, so it is not asked twice. Wherever the hub lands,
# konstruktor's own registry is what finds it again; `konstruktor list` shows them all.

# Compose's own rules, so the folder name and the identifier can stay the same string.
slugify() {
    printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9._-][^a-z0-9._-]*/-/g; s/^[^a-z0-9]*//; s/-*$//'
}

# The first free folder for this identifier: ~/MyHubs/lab-hub, then -2, -3, … Free means
# "not there", or there and empty — `hub create` refuses a folder that already holds a
# hub, so walking past those is the point.
free_hub_dir() {
    i=1
    while [ "$i" -le 20 ]; do
        if [ "$i" -eq 1 ]; then
            candidate="$HUB_PARENT/$1"
        else
            candidate="$HUB_PARENT/$1-$i"
        fi

        if [ ! -e "$candidate" ]; then
            printf '%s' "$candidate"
            return 0
        fi
        if [ -d "$candidate" ] && [ -z "$(ls -A "$candidate" 2>/dev/null)" ]; then
            printf '%s' "$candidate"
            return 0
        fi
        i=$((i + 1))
    done
    return 1
}

if [ "$RUN_AFTER" -eq 0 ]; then
    printf '\n'
    say "Run: mkdir -p $HUB_PARENT/my-hub && cd \$_ && $BIN_NAME hub create"
    printf '\n'
    exit 0
fi

# This script's own stdin is the curl pipe, so the wizard would otherwise read the rest of
# the script instead of the user. Re-opening the terminal is what makes the one-liner work.
if [ -r /dev/tty ] && [ -t 1 ]; then
    printf '\n'
    HUB_ID=""
    while [ -z "$HUB_ID" ]; do
        printf '  Hub identifier: ' >&2
        read -r HUB_ID_RAW < /dev/tty || die "no identifier given."
        HUB_ID="$(slugify "$HUB_ID_RAW")"
        [ -n "$HUB_ID" ] || say "! letters, digits, dot, underscore and dash — try again."
    done

    # An explicit --hub-dir is used verbatim; otherwise the first free ~/MyHubs/<id>.
    if [ -z "$HUB_DIR" ]; then
        HUB_DIR="$(free_hub_dir "$HUB_ID")" || die "no free folder for $HUB_ID under $HUB_PARENT — pass --hub-dir to say where it should go."
    fi

    printf '\n'
    say "Creating $HUB_ID in $HUB_DIR"
    # `hub create` makes the folder itself, so there is nothing to mkdir or cd into here.
    exec "$BIN" hub create --identifier "$HUB_ID" "$HUB_DIR" < /dev/tty
fi

# No terminal — a container, a CI job, a non-interactive shell. Installing and stopping is
# the right thing here; prompting into a pipe is not.
printf '\n'
say "No terminal attached, so nothing was created."
say "Run: mkdir -p $HUB_PARENT/my-hub && cd \$_ && $BIN_NAME hub create"
printf '\n'
