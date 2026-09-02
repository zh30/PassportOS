# IMPLEMENTATION_PLAN — PassportOS

## Stage 1: Board contract + input decoder
**Goal**: Official pin/bus/ADC windows in `passport-core`, millivolt → key, click vs long-press.
**Success Criteria**: Host tests map 0 / ~300 / ~595 / ~3300 using `{[0,150),[150,447),[447,1900)}`.
**Tests**: `crates/passport-core/tests/host_os.rs` millivolt + click/long-press.
**Status**: Complete

## Stage 2: Display + USB shell banner
**Goal**: ST7789P3 240×320 + PWM backlight + USB Serial/JTAG boot banner without reset loop.
**Success Criteria**: Boot log contains display init, shell ready, status bar (battery + radio).
**Tests**: Firmware links; on-device boot log twice.
**Status**: Complete (image linked; USB Serial/JTAG not enumerated at flash time)

## Stage 3: Compositor / launcher / workspaces
**Goal**: Status bar, launcher, non-overlap tiles, ≥2 workspaces, system menu, cheatsheet.
**Success Criteria**: Host tests for open/filter/activate, split vs single, workspace switch, app lifecycle.
**Tests**: `host_os.rs` launcher/tiles/workspace/lifecycle.
**Status**: Complete

## Stage 4: Peripherals
**Goal**: ES8311 play/record, CW2017 soft-fail, Wi-Fi scan, BLE advertise, light/deep sleep.
**Success Criteria**: Console/boot probes for I2C 0x18/0x63, wifi count, BLE name, audio worker, sleep.
**Tests**: Boot/console logs; host exclusive-resource tests.
**Status**: Complete (image linked; USB Serial/JTAG not enumerated at flash time)

## Stage 5: App contract + docs
**Goal**: Documented in-firmware app trait, sample `pulse` app, `docs/APP.md`.
**Success Criteria**: Docs name contract, pin map, coexistence, build/flash, register-an-app.
**Tests**: Docs section presence check; sample listed in launcher host test.
**Status**: Complete — superseded by Stage 7 unified API.

## Stage 6: Build + dual flash
**Goal**: ESP32-C3 image ≤ 3 MB factory; flash twice; identical primary boot lines.
**Success Criteria**: Link succeeds; both boot logs match; no panic loop.
**Tests**: `build.log`, `flash-N.log`, `boot-N.log`.
**Status**: Complete

## Stage 7: Unified App API (Passport Runtime)
**Goal**: Deep `App` + `Cx` API in `passport-core`; Pulse draws through it; developers add an app without editing drivers.
**Success Criteria**: Host tests for lifecycle, clipped draw, audio/radio Busy; firmware `PulseApp::draw` via `Cx`; `docs/APP.md` describes `App`/`Cx` not `dispatch_lifecycle`.
**Tests**: `crates/passport-core/tests/app_api.rs`; `cargo test -p passport-core`; firmware release link.
**Status**: Complete (API + Pulse/Tap/Flap on Cx; Flap best persisted via `Store` to KV flash at `0x350000`)

## Stage 8: Live compositor (no playfield flicker)
**Goal**: ST7789 GRAM is the framebuffer. Live frames are dirty rectangles inside `LIVE_SPI_BUDGET` (8 KiB / ~1.6 ms) so the 20 ms ADC period is never skipped. Flap OK/UP flaps on **Press**.
**Success Criteria**: `PaintPlan` live ticks have `wipe_rows()==0` and `game_live`; Flap live paint ≤ budget; full 240×298 fill exceeds budget (the flicker). Decoder Press(Ok) is the flap input.
**Tests**: `crates/passport-core/tests/flap.rs`, `host_os.rs` paint_plan_live_* / workspace_press_ok_reaches_flap.
**Status**: Complete

## Stage 9: System Wi-Fi pick + 3-key English IME
**Goal**: System **wifi** scans, lists APs, joins. Locked nets use a 1D-scan QWERTY IME (UP/DOWN/OK). Open nets skip the IME. Firmware keeps `WIFI` on a worker (scan then `connect_async`); UI never awaits radio.
**Success Criteria**: Host tests drive `Ime` / `Shell` for scan list, open-skip-IME, password Done → `SideEffect::WifiConnect`, long OK home. Firmware worker returns scan rows to the overlay and joins with `StationConfig`.
**Tests**: `crates/passport-core/tests/wifi.rs`; `cargo test -p passport-core`; firmware release link.
**Status**: Complete
