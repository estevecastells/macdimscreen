#!/bin/bash
# Install (or upgrade) the MacDimScreen daemon as a per-user launchd agent.
# No sudo: Night Shift is a per-user setting.
#
#   scripts/install.sh [--bin-dir DIR]
#
# --bin-dir  directory containing the `dimd` and `dimctl` binaries
#            (default: this script's directory if they are there, as inside the
#            app bundle; otherwise target/release of the checkout)
set -euo pipefail

LABEL="io.github.estevecastells.macdimscreen"
SUPPORT="${HOME}/Library/Application Support/MacDimScreen"
BIN="${SUPPORT}/bin"
PLIST="${HOME}/Library/LaunchAgents/${LABEL}.plist"
LOG_DIR="${HOME}/Library/Logs/MacDimScreen"
DOMAIN="gui/$(id -u)"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -x "${here}/dimd" ]]; then
  bin_dir="$here"
else
  bin_dir="${here}/../target/release"
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bin-dir) bin_dir="$2"; shift 2 ;;
    -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

if [[ $EUID -eq 0 ]]; then
  echo "Run install.sh as yourself, not root: the daemon is a per-user agent." >&2
  exit 1
fi
for b in dimd dimctl; do
  [[ -x "${bin_dir}/${b}" ]] || { echo "missing ${bin_dir}/${b}; run 'make build' first" >&2; exit 1; }
done

echo "==> Installing daemon to ${BIN}"
# Stop the running instance first; it restores Night Shift on SIGTERM.
# bootout returns before the agent has fully exited, and bootstrapping again
# too early fails ("Bootstrap failed: 5"), so wait until it's gone.
launchctl bootout "${DOMAIN}/${LABEL}" 2>/dev/null || true
for _ in $(seq 1 40); do
  launchctl print "${DOMAIN}/${LABEL}" >/dev/null 2>&1 || break
  sleep 0.25
done
mkdir -p "$BIN" "$LOG_DIR" "$(dirname "$PLIST")"
install -m 755 "${bin_dir}/dimd" "${bin_dir}/dimctl" "$BIN/"
install -m 755 "${here}/uninstall.sh" "${SUPPORT}/uninstall.sh"
# Binaries from a browser download carry a quarantine flag; launchd must not trip over it.
xattr -d com.apple.quarantine "${BIN}/dimd" "${BIN}/dimctl" 2>/dev/null || true

# Put dimctl on the PATH where we can without sudo.
for dir in /opt/homebrew/bin /usr/local/bin "${HOME}/.local/bin"; do
  if [[ -d "$dir" && -w "$dir" ]]; then
    ln -sf "${BIN}/dimctl" "${dir}/dimctl"
    echo "==> Linked ${dir}/dimctl"
    break
  fi
done

cat > "$PLIST" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>${LABEL}</string>
  <key>ProgramArguments</key>
  <array><string>${BIN}/dimd</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>LimitLoadToSessionType</key><string>Aqua</string>
  <key>ProcessType</key><string>Background</string>
  <key>StandardErrorPath</key><string>${LOG_DIR}/dimd.log</string>
  <key>StandardOutPath</key><string>${LOG_DIR}/dimd.log</string>
</dict>
</plist>
PLIST

echo "==> Starting ${LABEL}"
launchctl bootstrap "$DOMAIN" "$PLIST"
for _ in $(seq 1 20); do
  [[ -S "${SUPPORT}/dimd.sock" ]] && break
  sleep 0.25
done
if "${BIN}/dimctl" status; then
  echo "==> Installed. Log: ${LOG_DIR}/dimd.log"
else
  echo "The daemon didn't come up; see ${LOG_DIR}/dimd.log" >&2
  exit 1
fi
