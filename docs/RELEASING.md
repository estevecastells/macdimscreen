# Releasing

Releases are built and published by `.github/workflows/release.yml` when a version tag is pushed.

1. Bump `version` in the root `Cargo.toml` (for example `0.2.0`), then run `cargo build` to refresh `Cargo.lock`.
2. If the panel changed, refresh the README screenshots (light + dark):
   ```sh
   (cd app && swift run RenderScreenshots ../fixtures/status_response.json ../fixtures/config_response.json ../docs/images)
   ```
3. Check that `make ci` passes, then commit: `Release v0.2.0`.
4. Tag and push:
   ```sh
   git tag v0.2.0 && git push origin main v0.2.0
   ```
5. The workflow checks that the tag matches the Cargo version, runs the tests, builds the app and CLI, smoke-tests the CLI, and publishes a GitHub release with:
   - `MacDimScreen-macos-arm64.zip`: the menu bar app, bundling the daemon and installer
   - `macdimscreen-macos-arm64.tar.gz`: `dimd`, `dimctl`, `install.sh`, `uninstall.sh`
   - `SHA256SUMS`
   - `SHA256SUMS.sig`: Ed25519 signature checked by the in-app updater, made with the `UPDATE_SIGNING_KEY` secret. The workflow fails rather than publish an unsigned release.
6. Verify the release: `MDS_DOWNLOAD_ONLY=1 bash scripts/get.sh` downloads the latest release and checks its checksums without installing anything.

Asset names carry no version, so `releases/latest/download/<asset>` always resolves to the newest release. `scripts/get.sh` depends on this.

To build the same artifacts locally, run `scripts/package-release.sh` (output goes to `dist/`).

## Signing

Binaries are ad-hoc signed. Browser downloads of the app therefore need a one-time "Open Anyway" in System Settings. `get.sh` avoids this because `curl` doesn't set the quarantine flag. Signing with a Developer ID and notarizing would remove that step. That needs an Apple Developer Program membership, with the certificate and notary credentials stored as repository secrets.

## Rotating the signing key

Installed apps only accept a release signed by a key in `UpdateConfig.publicKeys`, so a new key needs one bridging release:

1. Generate a pair with `swift scripts/sign-update.swift --generate` and keep the private key somewhere safe outside GitHub (a secret can't be read back).
2. Put the new public key first in `UpdateConfig.publicKeys`, keeping the current one after it, and release as usual. That release is still signed with the current secret.
3. Once it's published, replace the `UPDATE_SIGNING_KEY` secret with the new private key. Later releases are signed with it, and the old public key can be dropped from the list.
