#!/bin/sh
# install-linux.sh — per-user, no-root XDG install for ZIM (ADR review
# §12.2: no root for normal operation, XDG everywhere, .desktop integration).
#
# Payload layout (exactly what the portable archive contains):
#   bin/zim  bin/zamind  bin/zamin
#   share/applications/mc.zamin.zim.desktop
#   share/icons/hicolor/...        (mc.zamin.zim.png / .svg)
#
# Usage:
#   install-linux.sh <payload-dir>         install (idempotent, upgrades too)
#   install-linux.sh --autostart on|off    toggle login autostart (no payload)
#   install-linux.sh --service on|off      toggle the systemd user unit for
#                                          the daemon (no payload; survives
#                                          logout, restarts on failure)
#   install-linux.sh --agent-service on|off
#                                          same for the remote agent — the
#                                          headless-box half of ADR-0011
#   install-linux.sh --uninstall           remove everything this script installed
#   install-linux.sh --prefix DIR          install root (default: $HOME/.local)
#
# Installed pieces (prefix $P, data $D = XDG_DATA_HOME, config $C = XDG_CONFIG_HOME):
#   $P/bin/zim  $P/bin/zamind  $P/bin/zamin  $P/bin/zaminagent
#   $D/applications/mc.zamin.zim.desktop        (Exec rewritten to $P/bin/zim)
#   $D/icons/hicolor/**/mc.zamin.zim.*
#   $C/autostart/mc.zamin.zim.desktop           (only via --autostart on)
#   $C/systemd/user/mc.zamin.daemon.service       (only via --service on)
#   $C/systemd/user/mc.zamin.agent.service        (only via --agent-service on)
#
# Never touched: daemon-owned state (XDG data zim/ tree, server roots).

set -eu

APP=mc.zamin.zim
PANEL_BIN=zim
BINS="zim zamind zamin zaminagent"

prefix="${HOME}/.local"
payload=""
action="install"
autostart=""
service=""
SERVICE_UNIT=mc.zamin.daemon.service
AGENT_UNIT=mc.zamin.agent.service

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
        --service)
            [ $# -ge 2 ] || die "--service needs on|off"
            case "$2" in
                on|off) action="service"; service="$2" ;;
                *) die "--service needs on|off" ;;
            esac
            shift
            ;;
        --agent-service)
            [ $# -ge 2 ] || die "--agent-service needs on|off"
            case "$2" in
                on|off) action="agent-service"; service="$2" ;;
                *) die "--agent-service needs on|off" ;;
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
            echo "Name=ZIM"
            echo "Comment=ZIM starts with your session so the daemon is ready"
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

# --- service toggles (independent of any payload) ---------------------------

# systemd is optional (musl boxes, containers, WSL1): the unit file is
# always managed honestly; the enable step is best-effort with a report.
service_systemctl() {
    command -v systemctl >/dev/null 2>&1 || return 1
    systemctl --user "$@"
}

# $1 unit, $2 description, $3 exec — one unit shape, two owners.
write_unit() {
    mkdir -p "${CONFIG}/systemd/user"
    {
        echo "[Unit]"
        echo "Description=$2"
        echo "Documentation=https://github.com/ZaminMC/ZIM"
        echo "StartLimitIntervalSec=60"
        echo "StartLimitBurst=4"
        echo ""
        echo "[Service]"
        echo "ExecStart=$3"
        echo "Restart=on-failure"
        echo "RestartSec=3"
        echo "PrivateTmp=true"
        echo "NoNewPrivileges=true"
        echo ""
        echo "[Install]"
        echo "WantedBy=default.target"
    } > "${CONFIG}/systemd/user/$1"
}

toggle_service() {
    # $1 on|off, $2 unit, $3 binary, $4 description
    unit_path="${CONFIG}/systemd/user/$2"
    if [ "$1" = "on" ]; then
        [ -x "${prefix}/bin/$3" ] ||
            die "no installed $3 at ${prefix}/bin/$3 — install first"
        write_unit "$2" "$4" "${prefix}/bin/$3"
        info "service unit written: $unit_path"
        if service_systemctl daemon-reload && \
            service_systemctl enable --now "$2"; then
            info "service ON: enabled and started (systemd user scope)"
        else
            info "warning: unit installed but not enabled — systemctl --user is"
            info "warning: unavailable or refused here (container/WSL/no bus). It"
            info "warning: activates on a real systemd user session."
        fi
    else
        service_systemctl disable --now "$2" 2>/dev/null || true
        [ -f "$unit_path" ] && rm -f "$unit_path"
        service_systemctl daemon-reload 2>/dev/null || true
        info "service OFF"
    fi
}

if [ "$action" = "service" ]; then
    toggle_service "$service" "$SERVICE_UNIT" zamind "zamind — the ZIM daemon"
    exit 0
fi

if [ "$action" = "agent-service" ]; then
    toggle_service "$service" "$AGENT_UNIT" zaminagent "zaminagent — the ZIM remote agent"
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
    for unit in "$SERVICE_UNIT" "$AGENT_UNIT"; do
        service_systemctl disable --now "$unit" 2>/dev/null || true
        [ -f "${CONFIG}/systemd/user/$unit" ] &&
            rm -f "${CONFIG}/systemd/user/$unit"
    done
    service_systemctl daemon-reload 2>/dev/null || true
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

info "installed: ${prefix}/bin/{zim,zamind,zamin,zaminagent}"
info "desktop:   ${DATA}/applications/${APP}.desktop"
info "icons:     ${DATA}/icons/hicolor (theme ${APP})"
info "autostart stays off until: install-linux.sh --autostart on"
info "daemon service stays off until: install-linux.sh --service on"
info "remote agent service stays off until: install-linux.sh --agent-service on"
