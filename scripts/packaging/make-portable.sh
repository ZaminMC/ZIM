#!/bin/sh
# make-portable.sh — assemble the Linux portable archive (§23 Phase 7).
#
# Layout is the contract install-linux.sh consumes, and it is runnable in
# place: ./bin/zim resolves its sibling zamind by construction.
#
# Payload layout:
#   zim-<version>/
#     bin/{zim,zamind,zamin,zaminagent}
#     share/applications/mc.zamin.zim.desktop
#     share/icons/hicolor/**/mc.zamin.zim.*
#     README.txt
#
# Usage: make-portable.sh <bins-dir> <version> <out-dir>
#   bins-dir: release binaries zim, zamind, zamin

set -eu

die() { echo "make-portable: $1" >&2; exit 1; }

[ $# -eq 3 ] || die "usage: make-portable.sh <bins-dir> <version> <out-dir>"
BINS="$1"
VERSION="$2"
OUT="$3"

HERE=$(CD=$(dirname "$0"); CD=$(cd "$CD" && pwd); echo "$CD")
REPO_ROOT=$(cd "$HERE/../.." && pwd)

for bin in zim zamind zamin zaminagent; do
    [ -f "$BINS/$bin" ] || die "bins-dir is missing $bin"
done

mkdir -p "$OUT"
ROOT=$(mktemp -d "$OUT/.portable-XXXXXX")
PAYLOAD="$ROOT/zim-$VERSION"

mkdir -p "$PAYLOAD/bin" "$PAYLOAD/share/applications"

for bin in zim zamind zamin zaminagent; do
    cp "$BINS/$bin" "$PAYLOAD/bin/$bin"
    chmod 755 "$PAYLOAD/bin/$bin"
done

cp "$REPO_ROOT/scripts/packaging/mc.zamin.zim.desktop" \
    "$PAYLOAD/share/applications/mc.zamin.zim.desktop"
for size in 32x32 128x128 256x256; do
    mkdir -p "$PAYLOAD/share/icons/hicolor/$size/apps"
    cp "$REPO_ROOT/apps/panel/src-tauri/icons/$size.png" \
        "$PAYLOAD/share/icons/hicolor/$size/apps/mc.zamin.zim.png"
done
mkdir -p "$PAYLOAD/share/icons/hicolor/scalable/apps"
cp "$REPO_ROOT/apps/panel/src-tauri/icons/icon.svg" \
    "$PAYLOAD/share/icons/hicolor/scalable/apps/mc.zamin.zim.svg"

cat > "$PAYLOAD/README.txt" <<'EOF'
ZIM — portable (Linux)
=============================

Run in place:  ./bin/zim

Desktop integration (per-user, no root):
  ./install-linux.sh .
  (adds ~/.local/bin commands, a launcher entry, and icons;
   --uninstall removes them; --autostart on starts the panel at login)

Contents:
  bin/zim   the desktop panel
  bin/zamind        the resident daemon (one per user; owns every server)
  bin/zamin         the CLI (zamin list | start | stop | logs -f | attach)

Daemon state lives under XDG_DATA_HOME/zim; server roots are the
ones you register. Uninstalling never touches daemon state or servers.
EOF
cp "$HERE/install-linux.sh" "$PAYLOAD/install-linux.sh"
chmod 755 "$PAYLOAD/install-linux.sh"

ARCH=x86_64
ARCHIVE="$OUT/ZIM-$VERSION-linux-$ARCH.tar.gz"
tar -C "$ROOT" -czf "$ARCHIVE" "zim-$VERSION"
rm -rf "$ROOT"
echo "make-portable: wrote $ARCHIVE"
