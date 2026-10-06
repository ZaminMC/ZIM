#!/bin/sh
# make-portable.sh — assemble the Linux portable archive (§23 Phase 7).
#
# Layout is the contract install-linux.sh consumes, and it is runnable in
# place: ./bin/zamin-panel resolves its sibling zamind by construction.
#
#   zaminpanel-<version>/
#     bin/{zamin-panel,zamind,zamin}
#     share/applications/mc.zamin.panel.desktop
#     share/icons/hicolor/**/mc.zamin.panel.*
#     README.txt
#
# Usage: make-portable.sh <bins-dir> <version> <out-dir>
#   bins-dir: release binaries zamin-panel, zamind, zamin

set -eu

die() { echo "make-portable: $1" >&2; exit 1; }

[ $# -eq 3 ] || die "usage: make-portable.sh <bins-dir> <version> <out-dir>"
BINS="$1"
VERSION="$2"
OUT="$3"

HERE=$(CD=$(dirname "$0"); CD=$(cd "$CD" && pwd); echo "$CD")
REPO_ROOT=$(cd "$HERE/../.." && pwd)

for bin in zamin-panel zamind zamin; do
    [ -f "$BINS/$bin" ] || die "bins-dir is missing $bin"
done

mkdir -p "$OUT"
ROOT=$(mktemp -d "$OUT/.portable-XXXXXX")
PAYLOAD="$ROOT/zaminpanel-$VERSION"

mkdir -p "$PAYLOAD/bin" "$PAYLOAD/share/applications"

for bin in zamin-panel zamind zamin; do
    cp "$BINS/$bin" "$PAYLOAD/bin/$bin"
    chmod 755 "$PAYLOAD/bin/$bin"
done

cp "$REPO_ROOT/scripts/packaging/mc.zamin.panel.desktop" \
    "$PAYLOAD/share/applications/mc.zamin.panel.desktop"
for size in 32x32 128x128 256x256; do
    mkdir -p "$PAYLOAD/share/icons/hicolor/$size/apps"
    cp "$REPO_ROOT/apps/panel/src-tauri/icons/$size.png" \
        "$PAYLOAD/share/icons/hicolor/$size/apps/mc.zamin.panel.png"
done
mkdir -p "$PAYLOAD/share/icons/hicolor/scalable/apps"
cp "$REPO_ROOT/apps/panel/src-tauri/icons/icon.svg" \
    "$PAYLOAD/share/icons/hicolor/scalable/apps/mc.zamin.panel.svg"

cat > "$PAYLOAD/README.txt" <<'EOF'
ZaminPanel — portable (Linux)
=============================

Run in place:  ./bin/zamin-panel

Desktop integration (per-user, no root):
  ./install-linux.sh .
  (adds ~/.local/bin commands, a launcher entry, and icons;
   --uninstall removes them; --autostart on starts the panel at login)

Contents:
  bin/zamin-panel   the desktop panel
  bin/zamind        the resident daemon (one per user; owns every server)
  bin/zamin         the CLI (zamin list | start | stop | logs -f | attach)

Daemon state lives under XDG_DATA_HOME/zaminpanel; server roots are the
ones you register. Uninstalling never touches daemon state or servers.
EOF
cp "$HERE/install-linux.sh" "$PAYLOAD/install-linux.sh"
chmod 755 "$PAYLOAD/install-linux.sh"

ARCH=x86_64
ARCHIVE="$OUT/ZaminPanel-$VERSION-linux-$ARCH.tar.gz"
tar -C "$ROOT" -czf "$ARCHIVE" "zaminpanel-$VERSION"
rm -rf "$ROOT"
echo "make-portable: wrote $ARCHIVE"
