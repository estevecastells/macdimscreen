## What and why

<!-- What does this change, and what problem does it solve? Link the issue. -->

## How it was tested

<!-- Tests added/updated. For display-facing changes, paste `dimctl status` output, your Mac model, macOS version and display(s). -->

## Checklist

- [ ] `make ci` passes locally (fmt, clippy -D warnings, tests, Swift checks, app build)
- [ ] Behaviour changes have unit tests (schedule logic in `dim-core`, daemon logic with the fakes in `dimd`)
- [ ] Protocol changes update `fixtures/` and both the Rust and Swift sides
- [ ] Display settings are still restored on exit and uninstall (Night Shift and the colour filter)
- [ ] No new dependencies, or each one is justified above

PRs are reviewed once a week; only green PRs are reviewed.
