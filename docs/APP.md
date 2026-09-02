# PassportOS app developer guide

PassportOS is a Rust firmware operating environment for the **FoloToy AI Passport**
(ESP32-C3, 8 MB flash, **no PSRAM**). It is not Linux, Hyprland, or Omarchy the
distro. The interaction model is Omarchy-like on this board: **button-only**
control, a **status bar**, a **unified launcher**, **non-overlapping tiled**
surfaces, **two workspaces**, a **system menu**, and a **button-map**.

Apps are compiled **into** the firmware. There is no MMU, no dynamic ELF/WASM
loader, and no untrusted third-party binary format.

## Hardware map (MCU-addressable)

All constants live in `crates/passport-core/src/board.rs`. Do not duplicate them.

| Function | Pin / bus | Notes |
| --- | --- | --- |
| Display ST7789P3 240×320 RGB565 | SPI2: MOSI **GPIO9**, SCLK **GPIO8**, CS **GPIO1**, DC **GPIO20**, 40 MHz, **mode 0**, invert-on | No MISO, no TE, no touch. RST is tied to 3.3 V (software `SWRESET`). |
| Backlight PWM | **GPIO21** LEDC 5 kHz 10-bit | UART0 TX defaults to GPIO21 — never use UART0 as the console. |
| Buttons UP / DOWN / OK | **GPIO0** ADC1_CH0 resistor ladder | Windows `{[0,150),[150,447),[447,1900)}` mV. Typical 0 / ~300 / ~595 / **3300 released**. Do **not** create a second ADC1 unit. Do **not** enable the internal pull-up. |
| I2C0 | SDA **GPIO10**, SCL **GPIO7** | Shared by ES8311 and CW2017. Reuse this bus; never `I2c::new` a second time on I2C0. |
| ES8311 codec | I2C **0x18** | Playback (DAC) + microphone (ADC). |
| I2S0 | MCLK **GPIO6**, BCLK **GPIO5**, WS **GPIO3**, DOUT **GPIO2**, DIN **GPIO4** | 16-bit stereo, 16 kHz, Philips I2S, MCU master, codec slave. Stream PCM; do not allocate a 96 KB capture buffer. |
| CW2017 fuel gauge | I2C **0x63** | Optional. Soft-fail if the chip NACKs. Charging bolt = USB-C SOF (host on the bus) or SOC rising. |
| USB Serial/JTAG | **GPIO18 / GPIO19** | Native ESP32-C3 USB. Console + flash. |
| Wi-Fi | 2.4 GHz STA scan + connect | System **wifi**: pick an AP. Open nets join immediately; locked nets open the 3-key English IME. |
| BLE | non-connectable advertising `PassportOS` | On demand. ESP32-C3 has **no Bluetooth Classic**. |
| Light / deep sleep | RTC timer wake | Deep sleep restarts the application. Unplugged **idle standby** (30 s, no keys) is only backlight PWM 0 — not this path. Any key restores brightness and is swallowed so it does not also fire UI. Charging (USB SOF / SOC-up) inhibits blanking. |

### NFC (NTAG213) — on the card, not on the MCU

The board has a **passive NTAG213** (144 byte user memory). Official BSP: *no MCU-facing API* — there is no I2C/SPI/GPIO to the die. Firmware **cannot** read or write the tag. Phones talk to it over RF.

PassportOS still treats NFC as a product surface:

- Launcher app **`nfc` / Tap** explains the tag and shows a sample URI.
- USB `nfc` prints `NTAG213 user=144B mcu=none`.
- `passport_core::nfc::encode_uri_tlv` builds Type 2 NDEF TLV bytes a **phone** can write (host-tested). `mcu_read` / `mcu_write` return `ApiError::NoBus`.
- `Cx::nfc()` returns the tag facts (`mcu_wired: false`).

### Not MCU-owned

- The dedicated **hardware power button** is **not a GPIO**. Firmware cannot read it.

Factory flash layout (do not move these if you want official Recovery restore):

| Region | Offset | Size |
| --- | --- | --- |
| factory app | `0x10000` | 3 MB |
| `cardid` | `0x356000` | 16 KB |
| Recovery | `0x700000` | 1 MB |

Flash with `espflash` **without** `--erase-all`.

## App contract (unified API)

PassportOS is a **compile-in app runtime**, not a loader. You implement
`passport_core::api::App` and receive capabilities through `Cx`. You never
touch GPIO, SPI, ADC windows, or DMA.

```rust
pub trait App {
    fn id(&self) -> AppId;
    fn name(&self) -> &'static str;   // launcher id, lowercase, ≤16
    fn title(&self) -> &'static str;  // desktop card title
    fn blurb(&self) -> &'static str;
    fn on_start(&mut self, cx: &mut Cx<'_>);
    fn on_stop(&mut self, cx: &mut Cx<'_>);
    fn on_focus(&mut self, cx: &mut Cx<'_>);
    fn on_blur(&mut self, cx: &mut Cx<'_>);
    fn on_key(&mut self, cx: &mut Cx<'_>, ev: ButtonEvent);
    fn on_tick(&mut self, cx: &mut Cx<'_>, dt_ms: u32);
    fn draw(&self, cx: &mut Cx<'_>, viewport: Rect);
}
```

Lifecycle hooks default to no-ops. `draw` is required.

`Cx` exposes `draw`, `audio`, `store`, `radio`, `power`, `battery`,
`brightness`, and `adc_mv()` (ladder voltage only — keys arrive via `on_key`).

**Invariants**

- Draw only inside `viewport`. The runtime clips to the tile; the status bar
  (22 px) is reserved.
- Long-press OK is home. Your app does not see that event.
- `cx.audio` / `cx.radio` return `ApiError::Busy` if Wi-Fi, BLE, or audio
  already owns the SRAM. Do not call `ExclusiveManager` yourself.
- No full-screen framebuffer. `Draw` is dirty rectangles over SPI.
  Live animation must stay inside `LIVE_SPI_BUDGET` (8 KiB ≈ 1.6 ms at 40 MHz).
  A 240×298 content fill is ~29 ms, flashes the ST7789, and starves the ADC.
- `Store` keys are per-app; never write `cardid` (`0x356000`) or Recovery
  (`0x700000`).

`ButtonEvent` is `Press` / `Release` / `Click` / `LongPress` of `Up` / `Down` /
`Ok`. Press settle is 30 ms (the ladder walks through OK). **Release settle is
one input sample (5 ms)** so rapid OK taps are not swallowed. A click is
emitted only when the pin returns to released — walking OK → DOWN → UP does
not click OK. USB console `key …` uses the same decoder as GPIO0. The firmware
samples ADC every `INPUT_TICK_MS` (5 ms) and again after SPI so a paint cannot
skip a tap.

## Input map (Omarchy-like, three keys)

| Action | Binding |
| --- | --- |
| Open/close launcher | **OK long** (Super analogue) |
| Move selection / focus | **UP / DOWN** click |
| Activate | **OK** click |
| In **Flap** | **OK** or **UP** **press** flaps (not click-on-release) |
| In **Stack** | **OK** **press** drops the sliding slab |
| In **Brick** | **UP / DOWN** move the paddle, **OK** **press** serves |
| In a game | UP/DOWN (click or long) stay in the game — they do not switch tiles or workspaces. **OK long** is still home. |
| Workspace 1 | **UP long** (not while a game is focused) |
| Workspace 2 | **DOWN long** (not while a game is focused) |
| System menu | launcher item `system` |
| Keys / About | system menu items |
| Wi-Fi | system **wifi** — UP/DN pick AP, OK join. Locked: IME (UP/DN move, OK type, **go** submit, **x** back). Open nets skip the IME. |
| Light / Dark | system menu **appearance** |

No pointer, no Super-key chords, no overlapping windows.

## How to add an app

1. Create `firmware/src/apps/foo.rs` implementing `App`.
2. Add a field on `firmware/src/apps/mod.rs` `Apps` and construct it in `Apps::new`.
3. Register it: `shell.register_app(apps.foo.id(), apps.foo.name())?`.
4. Host-test logic in `crates/passport-core` (see `tests/app_api.rs` for the
   `HelloApp` pattern). Do not put HAL types in core.
5. Rebuild and flash (see below).

Do **not** edit `st7789.rs` or ADC setup. Samples: **`pulse`**, **`flap`**,
**`stack`**, and **`brick`**. Game logic lives in `crates/passport-core`.

**Flap**: OK or UP **press** flaps. **Stack**: OK **press** drops the slab.
**Brick**: paddle on the right; UP/DOWN move it, OK serves; miss is down.
Long OK is home. Bests are stored in the 4 KB KV page at `0x350000` (not
`cardid`, not Recovery). Playing ticks send sprite patches only.

## USB console (same event path as the keys)

| Command | Effect |
| --- | --- |
| `status` | Status bar: **time**, **battery** and **radio** plus workspace/app |
| `time` / `time HH:MM` | Read / set local clock (no NTP; last set is stored in KV) |
| `apps` | Registered apps (`*` running, `+` focused) |
| `launcher` / `filter <prefix>` / `activate [name]` | Launcher |
| `key mv <mV> [ticks]` | Raw millivolt inject through the decoder |
| `key up\|down\|ok click\|long` | Synthesized via typical 0 / 300 / 595 / 3300 mV |
| `workspace 0\|1` | Switch workspace |
| `brightness 0-100` | PWM backlight |
| `radio wifi\|ble\|off` | Wi-Fi overlay (scan/pick/IME/join) / BLE advertise / radio off |
| `sleep light\|deep` | RTC-wake light 2 s / deep 5 s |
| `audio beep\|rec` | Playback / microphone path |
| `probe` | I2C probe `0x18` and `0x63` |
| `keys` / `about` / `menu` / `help` | Cheatsheet / about / system menu / help |
| `theme dark\|light\|toggle` | Appearance. Boot is **dark** (light full-panel fills 花屏 on this ST7789). |

## Build and flash

Host tests (decoder, tiler, launcher, workspaces, registry):

```bash
cargo test -p passport-core
```

Firmware (ESP32-C3, 8 MB, factory slot 3 MB):

```bash
cargo build -p passport-os --release --target riscv32imc-unknown-none-elf
espflash flash --flash-size 8mb --partition-table partitions.csv \
  --monitor target/riscv32imc-unknown-none-elf/release/passport-os
```

Do not erase `0x356000` or `0x700000`. A successful boot prints the same primary
lines every time:

```
[boot] PassportOS r1
[boot] display ST7789P3 240x320 invert-on bl=80
[shell] ready workspaces=2 overlay=none
[status] bat=… radio=… ws=0 app=shell bl=80
```

No panic/reset loop. `bat=--` is valid when CW2017 is absent.

## RAM budget

- No PSRAM.
- Status + tiled UI uses line/glyph SPI windows, not a 240×320 framebuffer.
- Heap is ~72 KB for the radio stacks.
- Wi-Fi scan, BLE advertise, and I2S DMA are started on demand and treated as
  exclusive with `ExclusiveManager`.
