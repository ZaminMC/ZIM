#!/bin/sh
# install-linux.sh — per-user, no-root XDG install for ZaminPanel (ADR review
# §12.2: no root for normal operation, XDG everywhere, .desktop integration).
#
# Payload layout (exactly what the portable archive contains):
#   bin/zamin-panel  bin/zamind  bin/zamin
#   share/applications/mc.zamin.panel.desktop
#   share/icons/hicolor/...        (mc.zamin.panel.png / .svg)
#
# Usage:
#   install-linux.sh <payload-dir>         install (idempotent, upgrades too)
#   install-linux.sh --autostart on|off    toggle login autostart (no payload)
#   install-linux.sh --uninstall           remove everything this script installed
#   install-linux.sh --prefix DIR          install root (default: $HOME/.local)
#
# Installed pieces (prefix $P, data $D = XDG_DATA_HOME, config $C = XDG_CONFIG_HOME):
#   $P/bin/zamin-panel  $P/bin/zamind  $P/bin/zamin
#   $D/applications/mc.zamin.panel.desktop        (Exec rewritten to $P/bin/zamin-panel)
#   $D/icons/hicolor/**/mc.zamin.panel.*
#   $C/autostart/mc.zamin.panel.desktop           (only via --autostart on)
#
# Never touched: daemon-owned state (XDG data zaminpanel/ tree, server roots).

set -eu

APP=mc.zamin.panel
PANEL_BIN=zamin-panel
BINS="zamin-panel zamind zamin"

prefix="${HOME}/.local"
payload=""
action="install"
autostart=""

die() { echo "install-linux: $1" >&2; exit 1; }
info() { echo "install-linux: $1"; }

xdg_data() {
    if [ -n "${XDG_DATA_HOME:-}" ]; then printf '%s' "${XDG_DATA_HOME}"
    else printf '%s' "${HOME}/.local/share"; fi
}
xdg_config() {
    if [ -n "${XDG_CONFIG_HOME:-}" ]; then printf '%s' "${XDG_CONFIG_HOME}"
    else printf '%s' "${HOME}/.config"; fi
}

# --- argument parsing ------------------------------------------------------
while [ $# -gt 0 ]; do
    case "$1" in
        --uninstall) action="uninstall" ;;
        --autostart)
            [ $# -ge 2 ] || die "--autostart needs on|off"
            case "$2" in
                on|off) action="autostart"; autostart="$2" ;;
                *) die "--autostart needs on|off" ;;
            esac
            shift
            ;;
        --prefix)
            [ $# -ge 2 ] || die "--prefix needs a directory"
            prefix="$2"
            shift
            ;;
        -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
        -*) die "unknown option $1 (see --help)" ;;
        *)
            [ -z "$payload" ] || die "exactly one payload directory is expected"
            payload="$1"
            ;;
    esac
    shift
done

DATA="$(xdg_data)"
CONFIG="$(xdg_config)"

# --- autostart toggle (independent of any payload) --------------------------
autostart_path="${CONFIG}/autostart/${APP}.desktop"

if [ "$action" = "autostart" ]; then
    if [ "$autostart" = "on" ]; then
        [ -x "${prefix}/bin/${PANEL_BIN}" ] ||
            die "no installed panel at ${prefix}/bin/${PANEL_BIN} — install first"
        mkdir -p "${CONFIG}/autostart"
        {
            echo "[Desktop Entry]"
            echo "Type=Application"
            echo "Name=ZaminPanel"
            echo "Comment=ZaminPanel starts with your session so the daemon is ready"
            echo "Exec=${prefix}/bin/${PANEL_BIN}"
            echo "Icon=${APP}"
            echo "Terminal=false"
            echo "X-GNOME-Autostart-enabled=true"
        } > "$autostart_path"
        info "autostart ON: $autostart_path"
    else
        [ -f "$autostart_path" ] && rm -f "$autostart_path"
        info "autostart OFF"
    fi
    exit 0
fi

# --- uninstall --------------------------------------------------------------
if [ "$action" = "uninstall" ]; then
    for bin in $BINS; do
        [ -f "${prefix}/bin/${bin}" ] && rm -f "${prefix}/bin/${bin}"
    done
    [ -f "${DATA}/applications/${APP}.desktop" ] &&
        rm -f "${DATA}/applications/${APP}.desktop"
    if [ -d "${DATA}/icons/hicolor" ]; then
        find "${DATA}/icons/hicolor" -name "${APP}.*" -exec rm -f {} +
    fi
    [ -f "$autostart_path" ] && rm -f "$autostart_path"
    command -v update-desktop-database >/dev/null 2>&1 &&
        update-desktop-database "${DATA}/applications" 2>/dev/null || true
    info "uninstalled from ${prefix} and ${DATA} (daemon data was not touched)"
    exit 0
fi

# --- install ----------------------------------------------------------------
[ -n "$payload" ] || die "usage: install-linux.sh <payload-dir> (see --help)"
[ -d "$payload" ] || die "payload directory not found: $payload"
for bin in $BINS; do
    [ -f "${payload}/bin/${bin}" ] || die "payload is missing bin/${bin}"
done

mkdir -p "${prefix}/bin" "${DATA}/applications"

# Binaries: copy (not symlink) so a deleted/moved archive never breaks the install.
for bin in $BINS; do
    cp -f "${payload}/bin/${bin}" "${prefix}/bin/${bin}"
    chmod 755 "${prefix}/bin/${bin}"
done

# Desktop entry with Exec pointed at the installed binary.
sed "s|^Exec=.*|Exec=${prefix}/bin/${PANEL_BIN}|" \
    "${payload}/share/applications/${APP}.desktop" \
    > "${DATA}/applications/${APP}.desktop"

# Icons: the hicolor subtree, verbatim.
if [ -d "${payload}/share/icons/hicolor" ]; then
    mkdir -p "${DATA}/icons"
    cp -R "${payload}/share/icons/hicolor" "${DATA}/icons/"
fi

# Best-effort cache refreshes (absent tools or headless boxes are fine).
command -v update-desktop-database >/dev/null 2>&1 &&
    update-desktop-database "${DATA}/applications" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null 2>&1 &&
    gtk-update-icon-cache -q -t -f "${DATA}/icons/hicolor" 2>/dev/null || true

info "installed: ${prefix}/bin/{zamin-panel,zamind,zamin}"
info "desktop:   ${DATA}/applications/${APP}.desktop"
info "icons:     ${DATA}/icons/hicolor (theme ${APP})"
info "autostart stays off until: install-linux.sh --autostart on"
