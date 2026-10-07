#!/bin/bash
# Remove the MacDimScreen daemon. Night Shift goes back to how it was before.
#
#   uninstall.sh [--purge]     --purge also deletes settings and logs
set -euo pipefail

LABEL="io.github.estevecastells.macdimscreen"
SUPPORT="${HOME}/Library/Application Support/MacDimScreen"
PLIST="${HOME}/Library/LaunchAgents/${LABEL}.plist"
DOMAIN="gui/$(id -u)"

# SIGTERM makes dimd restore Night Shift's previous settings before exiting.
launchctl bootout "${DOMAIN}/${LABEL}" 2>/dev/null || true
for _ in $(seq 1 40); do
  launchctl print "${DOMAIN}/${LABEL}" >/dev/null 2>&1 || break
  sleep 0.25
done
rm -f "$PLIST"
for dir in /opt/homebrew/bin /usr/local/bin "${HOME}/.local/bin"; do
  [[ -L "${dir}/dimctl" ]] && rm -f "${dir}/dimctl"
done
rm -rf "${SUPPORT:?}/bin" "${SUPPORT:?}/dimd.sock"
if [[ "${1:-}" == "--purge" ]]; then
  rm -rf "$SUPPORT" "${HOME}/Library/Logs/MacDimScreen"
  echo "MacDimScreen removed, including settings."
else
  echo "MacDimScreen daemon removed. Settings kept in ${SUPPORT}."
fi
