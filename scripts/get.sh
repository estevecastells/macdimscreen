#!/bin/bash
# Install or upgrade MacDimScreen from a GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/estevecastells/macdimscreen/main/scripts/get.sh | bash
#
# Environment options:
#   MDS_VERSION=v0.1.0    install a specific release (default: latest)
#   MDS_NO_APP=1          install only the background service and CLI, not the menu bar app
#   MDS_DOWNLOAD_ONLY=1   download and verify into a temp dir, then stop
#
# Downloads with curl don't get macOS's quarantine flag, so the unsigned app
# opens without a Gatekeeper prompt. Checksums are verified before anything runs.
set -euo pipefail

REPO="estevecastells/macdimscreen"
APP_ZIP="MacDimScreen-macos-arm64.zip"
CLI_TGZ="macdimscreen-macos-arm64.tar.gz"

say() { printf '\033[1m==> %s\033[0m\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || die "MacDimScreen only runs on macOS"
[[ "$(uname -m)" == "arm64" ]] || die "MacDimScreen release builds are for Apple Silicon; build from source on Intel"
major="$(sw_vers -productVersion | cut -d. -f1)"
(( major >= 14 )) || die "macOS 14 or later is required"

version="${MDS_VERSION:-latest}"
if [[ "$version" == "latest" ]]; then
  base="https://github.com/${REPO}/releases/latest/download"
else
  base="https://github.com/${REPO}/releases/download/${version}"
fi

tmp="$(mktemp -d)"
if [[ -z "${MDS_DOWNLOAD_ONLY:-}" ]]; then
  trap 'rm -rf "$tmp"' EXIT
fi

say "Downloading MacDimScreen (${version})"
for f in SHA256SUMS "$CLI_TGZ" "$APP_ZIP"; do
  curl -fSL --progress-bar -o "${tmp}/${f}" "${base}/${f}" || die "download failed: ${base}/${f}"
done

say "Verifying checksums"
(cd "$tmp" && shasum -a 256 -c SHA256SUMS) || die "checksum mismatch; aborting"

tar -xzf "${tmp}/${CLI_TGZ}" -C "$tmp"

if [[ -n "${MDS_DOWNLOAD_ONLY:-}" ]]; then
  say "Downloaded and verified in ${tmp}"
  exit 0
fi

if pgrep -x Flux >/dev/null; then
  say "Quitting f.lux (it would fight over the screen colour)"
  osascript -e 'quit app "Flux"' 2>/dev/null || pkill -x Flux || true
fi

say "Installing the background service"
/bin/bash "${tmp}/macdimscreen/install.sh" --bin-dir "${tmp}/macdimscreen"

if [[ -z "${MDS_NO_APP:-}" ]]; then
  dest="/Applications"
  [[ -w "$dest" ]] || dest="${HOME}/Applications"
  mkdir -p "$dest"
  say "Installing the menu bar app to ${dest}"
  pkill -x MacDimScreen 2>/dev/null || true
  rm -rf "${dest}/MacDimScreen.app"
  ditto -x -k "${tmp}/${APP_ZIP}" "$dest"
  open "${dest}/MacDimScreen.app"
fi

say "Done. Try: dimctl status"
