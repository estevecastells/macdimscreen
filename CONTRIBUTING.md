# Contributing

Thanks for helping. MacDimScreen changes how every pixel on the screen looks, so changes need to be predictable and reversible. In return, the process is predictable too.

## Review cadence

- **Pull requests and issues are triaged once a week.** Expect a first response within 7 days.
- Only PRs that are **green on CI** and meet the checklist below get reviewed.
- Small, focused PRs get merged fastest. For larger changes (new display mechanisms, protocol changes, new dependencies), open an issue first so we can agree on the approach.

## Merge requirements

Every PR must:

1. **Pass CI.** That means `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, the Swift protocol checks, shellcheck, and building the app bundle. Run `make ci` locally before you push.
2. **Include tests** for behaviour changes:
   - Schedule, sun or config logic: a unit test in `crates/dim-core`.
   - Daemon behaviour (what gets sent to Night Shift or the colour filter, and when): a test with the fakes in `crates/dimd/src/lib.rs`.
   - Protocol changes: update `fixtures/`, and both the Rust (`protocol.rs`) and Swift (`KitChecks`) sides.
3. **Keep display changes reversible.** Night Shift and the colour filter must be restored on exit and uninstall, and a colour filter the user set up must be left alone unless Extra warmth is in use.
4. **Not add dependencies lightly.** Justify each one in the PR description. GitHub Actions must be pinned to a full commit SHA, with the version in a comment.
5. **Be explained.** Say what changed, why, and how you tested it. For display-facing changes, include your Mac model, macOS version and display(s).

## Private APIs

Night Shift (`CBBlueLightClient` in CoreBrightness) and the colour filter (`MADisplayFilterPref*` in MediaAccessibility) are private. Call them only through `nightshift.rs` and `colorfilter.rs`, always cast to the exact C signature (an untyped `objc_msgSend` passes floats in the wrong registers), and document any behaviour you measure, and how you measured it. `log stream --predicate 'process == "corebrightnessd"'` shows every white-point change Night Shift makes (`WP update: BLR`), which lets you test without relying on your eyes.

## Code style

- Match the surrounding code. Rust is formatted by `rustfmt.toml` (120 columns). Swift follows standard Swift API guidelines.
- Comments explain *why*, not *what*.
- Keep `dim-core` free of OS calls so it stays unit-testable.

## Reporting security issues

See [SECURITY.md](SECURITY.md). Please don't open public issues for vulnerabilities.
