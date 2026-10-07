# Security policy

`dimd` runs as your user (a launchd agent, never root) and accepts requests over a local Unix socket.

## Reporting

Please report vulnerabilities privately via [GitHub security advisories](https://github.com/estevecastells/macdimscreen/security/advisories/new), not public issues. Reports are reviewed with the weekly triage, or sooner for anything severe.

## Threat model (summary)

- The socket lives in `~/Library/Application Support/MacDimScreen/` with mode `0600`, and the daemon drops any connection whose peer uid (checked with `getpeereid`) isn't its own.
- The daemon only changes Night Shift (CoreBrightness) and the Accessibility colour filter, both per-user settings, and puts them back on exit and uninstall.
- Requests are size-limited JSON lines. Malformed input gets an error response; settings are validated before they're applied or saved.
