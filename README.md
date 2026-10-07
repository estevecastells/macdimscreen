<div align="center">

<h1>MacDimScreen</h1>

<p><strong>Free, open-source f.lux alternative for macOS that works on M5 Macs.</strong><br/>
Warmer, dimmer screen after sunset, on built-in and external displays.</p>

<a href="https://github.com/estevecastells/macdimscreen/releases/latest/download/MacDimScreen-macos-arm64.zip"><img src="https://img.shields.io/badge/Download_for_Mac-Apple_Silicon-0A84FF?style=for-the-badge&logo=apple&logoColor=white" alt="Download MacDimScreen for Mac (Apple Silicon)" height="44"/></a>
&nbsp;
<a href="https://github.com/estevecastells/macdimscreen/releases/latest/download/macdimscreen-macos-arm64.tar.gz"><img src="https://img.shields.io/badge/Command_line-.tar.gz-24292F?style=for-the-badge&logo=gnubash&logoColor=white" alt="Download the command-line tools" height="44"/></a>

<p>
<a href="https://github.com/estevecastells/macdimscreen/releases/latest"><img src="https://img.shields.io/github/v/release/estevecastells/macdimscreen?label=latest&style=flat-square" alt="Latest release"/></a>
<img src="https://img.shields.io/badge/macOS-14%2B-555?style=flat-square&logo=apple" alt="macOS 14 or later"/>
<a href="https://github.com/estevecastells/macdimscreen/actions/workflows/ci.yml"><img src="https://github.com/estevecastells/macdimscreen/actions/workflows/ci.yml/badge.svg" alt="CI"/></a>
<a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" alt="MIT license"/></a>
</p>

**Or install in one line** (no password, verifies checksums):

</div>

```sh
curl -fsSL https://raw.githubusercontent.com/estevecastells/macdimscreen/main/scripts/get.sh | bash
```

<div align="center"><sub>
Requires macOS 14+. After downloading, move the app to Applications, open it, and click <b>Install Background Service</b> in the menu bar.<br/>
The app isn't notarized yet: on first launch, open <b>System Settings → Privacy &amp; Security</b> and click <b>Open Anyway</b>. Quit f.lux first.
</sub></div>

---

## Why

f.lux and most screen-colour apps change the display's gamma tables (`CGSetDisplayTransferByTable`). On M5 Pro, M5 Max and MacBook Neo Macs running macOS 26, those calls report success but the display ignores them, on both built-in and external screens ([Apple developer forums](https://developer.apple.com/forums/thread/819331), FB22273730). f.lux still runs, but the screen doesn't change.

MacDimScreen does the same job through the parts of macOS that still work there:

| | What it uses | Why |
|---|---|---|
| **Colour temperature** | Night Shift's engine (CoreBrightness), set to an exact kelvin value | Applied in the display pipeline, so every app is tinted evenly |
| **Extra warmth** (optional) | Accessibility Color Tint filter, amber, at the strength you choose | Night Shift stops at 2700 K and looks milder than f.lux at the same number |
| **Dimming** (optional) | A click-through black overlay on every screen | Black at opacity *a* multiplies brightness by (1 − *a*): exact, in every app |

Custom ColorSync profiles were tried too and rejected: apps that colour-manage themselves (Chrome, Electron apps) cancel or double the shift, so some windows end up orange and others white.

On Macs where gamma tables still work, MacDimScreen works the same way. It doesn't depend on the bug.

## What it does

- **Follows the sun.** Starts warming at sunset and reaches your night colour over a transition (20–90 min, default 40). In the morning it's back to daylight colours by your wake time, or by sunrise if you don't set one.
- **Imports your f.lux settings** on first run: location, night colour temperature and wake time.
- **Menu bar panel** with Schedule / Pause 1 h / Off, night colour, dimming, extra warmth, transition length, wake time and location. Dragging a slider previews the result live, even during the day.
- **`dimctl`** for the terminal: `dimctl status`, `dimctl plan` (today's curve), `dimctl pause 30`, `dimctl manual 2700 --dim 20`, `dimctl set --night 3000 --tint 40`.
- **Leaves things as it found them.** Night Shift's and the colour filter's previous settings are saved when the service starts and restored when it stops or is uninstalled. If you have your own colour filter set up, it's only touched while Extra warmth is in use.
- **Light.** A small Rust background service that wakes every 15 s and only talks to macOS when something needs to change.

## How it works

```
 menu bar app (Swift) ──JSON over Unix socket──▶ dimd (Rust, runs as you, launchd agent)
   └ dimming overlay                               │  sun position, schedule, config
                                                   ├─▶ Night Shift (CoreBrightness): colour temperature
 dimctl (CLI) ─────────────────────────────────────┘  └─▶ Color Filters (MediaAccessibility): extra warmth
```

Every 15 seconds the daemon:

1. Works out the sun's position for your location (NOAA-style solar ephemeris) and today's sunrise, sunset and wake time.
2. Computes the night factor, from 0 (day) to 1 (night), with smoothstep easing across each transition, and interpolates the colour temperature in mireds (1e6 / K), which is close to perceptually even.
3. Sets Night Shift to that temperature in manual mode, so its own schedule doesn't interfere. It only sends a change when the temperature moves by 10 K or more, and Night Shift fades each change over about 2 s, so transitions look smooth. If something else changes Night Shift, the daemon sets it back on the next tick.
4. Applies extra warmth when its share of the night factor reaches 25 %, the lowest intensity macOS allows for the tint.
5. Publishes the dimming level, which the menu bar app applies with its overlay.

Settings live in `~/Library/Application Support/MacDimScreen/config.toml`:

```toml
latitude = 41.39
longitude = 2.17
day_kelvin = 6500.0        # 6500 = unchanged
night_kelvin = 3400.0      # Night Shift's range is 2700–6000; lower values are clamped
night_dim_pct = 0.0        # overlay dimming at night, 0–90
night_tint_pct = 40.0      # extra warmth: 0 = off, else 25–100
transition_minutes = 40.0
wake_time = "07:30"        # omit to follow sunrise

[mode]
kind = "auto"              # auto | off | paused (until = unix time) | manual (kelvin, dim_pct, tint_pct)
```

## Install

**One line** (recommended): see the command at the top. It downloads the latest release, checks the checksums, quits f.lux if it's running, installs the background service and the app, and opens it.

**App only.** Download the zip, move MacDimScreen.app to Applications, open it and click **Install Background Service**.

**From source:**

```sh
xcode-select --install          # Swift toolchain; full Xcode isn't needed
make install                    # builds dimd + dimctl, installs the launchd agent (no sudo)
make run-app                    # builds and opens build/MacDimScreen.app
```

**Uninstall:** `~/Library/Application\ Support/MacDimScreen/uninstall.sh` (add `--purge` to also delete settings and logs), then delete the app. Night Shift and your colour filter go back to how they were.

Log: `~/Library/Logs/MacDimScreen/dimd.log`.

## Limitations

- Night Shift can't go warmer than 2700 K. Use Extra warmth to go further.
- The colour tint switches on at 25 % rather than fading in from zero, because macOS won't set a lower intensity. The evening transition therefore has one small visible step.
- When the daemon first takes over Night Shift from its own sunset schedule, the screen may go neutral for a moment before warming.
- The dimming overlay is drawn by the menu bar app, so it disappears if you quit the app. Colour temperature keeps working without it.
- CoreBrightness and the colour-filter functions are private macOS APIs. A future macOS update could change them. The daemon reports errors in the panel and in `dimctl status` rather than failing silently.
- Not notarized yet (see Install).

## Development

```sh
make ci     # fmt, clippy -D warnings, Rust tests, Swift protocol checks, app bundle
```

- `crates/dim-core`: sun position, schedule, config and protocol. No OS calls, fully unit-tested.
- `crates/dimd`: the daemon, with the Night Shift and colour-filter bridges (`nightshift.rs`, `colorfilter.rs`). Daemon logic is tested against fakes.
- `crates/dimctl`: the CLI.
- `app/`: the SwiftPM menu bar app. `KitChecks` checks the Swift models against the same JSON fixtures as the Rust tests (`fixtures/`).

`dimd --dry-run --once` prints what the schedule wants right now without touching the display. See [CONTRIBUTING.md](CONTRIBUTING.md) for the review process.

## License

[MIT](LICENSE). Not affiliated with f.lux or Apple. They're mentioned only for comparison.
