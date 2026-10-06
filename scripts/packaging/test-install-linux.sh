#!/bin/sh
# test-install-linux.sh — round-trip test for install-linux.sh, run in CI
# and locally. Builds a stub payload (fake binaries, the real share tree),
# installs into a sandboxed HOME, asserts the layout, toggles autostart,
# and uninstalls. Exits nonzero on the first broken promise.
#
# The sandbox is fully self-contained: XDG_DATA_HOME / XDG_CONFIG_HOME /
# HOME all point into a fresh temp directory — nothing on the real machine
# is read or written.

set -eu

HERE=$(CD=$(dirname "$0"); CD=$(cd "$CD" && pwd); echo "$CD")
REPO_ROOT=$(cd "$HERE/../.." && pwd)
INSTALL="$HERE/install-linux.sh"

[ -x "$INSTALL" ] || { echo "install-linux.sh missing" >&2; exit 1; }

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

HOME="$WORK/home"
export HOME
XDG_DATA_HOME="$WORK/home/.local/share"
export XDG_DATA_HOME
XDG_CONFIG_HOME="$WORK/home/.config"
export XDG_CONFIG_HOME
PREFIX="$WORK/home/.local"

PAYLOAD="$WORK/payload"
mkdir -p "$PAYLOAD/bin" "$PAYLOAD/share/applications" \
    "$XDG_DATA_HOME" "$XDG_CONFIG_HOME"

# Stub binaries: exit 0 when executed.
for bin in zamin-panel zamind zamin; do
    printf '#!/bin/sh\nexit 0\n' > "$PAYLOAD/bin/$bin"
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

fail() { echo "test-install-linux: FAIL: $1" >&2; exit 1; }
ok() { echo "test-install-linux: ok — $1"; }

# 1. install
"$INSTALL" "$PAYLOAD" > "$WORK/install.log" 2>&1 || fail "install exited nonzero"
for bin in zamin-panel zamind zamin; do
    [ -x "$PREFIX/bin/$bin" ] || fail "$bin not installed/executable"
done
ok "binaries installed to \$prefix/bin"

DESKTOP="$XDG_DATA_HOME/applications/mc.zamin.panel.desktop"
[ -f "$DESKTOP" ] || fail "desktop entry missing"
grep -q "^Exec=$PREFIX/bin/zamin-panel$" "$DESKTOP" ||
    fail "desktop Exec was not rewritten to the installed path"
ok "desktop entry installed with Exec rewritten"

for icon in \
    "icons/hicolor/32x32/apps/mc.zamin.panel.png" \
    "icons/hicolor/128x128/apps/mc.zamin.panel.png" \
    "icons/hicolor/256x256/apps/mc.zamin.panel.png" \
    "icons/hicolor/scalable/apps/mc.zamin.panel.svg"; do
    [ -f "$XDG_DATA_HOME/$icon" ] || fail "icon missing: $icon"
done
ok "hicolor icons installed"

# 2. idempotent upgrade
"$INSTALL" "$PAYLOAD" > "$WORK/upgrade.log" 2>&1 || fail "re-install exited nonzero"
ok "re-install (upgrade) is idempotent"

# 3. autostart on/off
"$INSTALL" --autostart on > "$WORK/autostart.log" 2>&1 || fail "autostart on failed"
AUTO="$XDG_CONFIG_HOME/autostart/mc.zamin.panel.desktop"
[ -f "$AUTO" ] || fail "autostart entry missing"
grep -q "^Exec=$PREFIX/bin/zamin-panel$" "$AUTO" || fail "autostart Exec wrong"
"$INSTALL" --autostart off > "$WORK/autostart-off.log" 2>&1 || fail "autostart off failed"
[ ! -f "$AUTO" ] || fail "autostart entry not removed"
ok "autostart on/off writes and removes the XDG entry"

# 4. missing payload piece is a hard error
mkdir -p "$WORK/broken/bin"
if "$INSTALL" "$WORK/broken" > "$WORK/broken.log" 2>&1; then
    fail "missing-bin payload was accepted"
fi
grep -q "missing bin/zamin-panel" "$WORK/broken.log" ||
    fail "missing-bin error is not the honest one"
ok "a broken payload is rejected with a typed message"

# 5. uninstall
"$INSTALL" "$PAYLOAD" > "$WORK/reinstall.log" 2>&1
"$INSTALL" --uninstall > "$WORK/uninstall.log" 2>&1 || fail "uninstall failed"
for bin in zamin-panel zamind zamin; do
    [ ! -f "$PREFIX/bin/$bin" ] || fail "$bin survived uninstall"
done
[ ! -f "$DESKTOP" ] || fail "desktop entry survived uninstall"
[ ! -f "$XDG_DATA_HOME/icons/hicolor/scalable/apps/mc.zamin.panel.svg" ] ||
    fail "icons survived uninstall"
ok "uninstall removes exactly what install added"

echo "test-install-linux: ALL PASS"
