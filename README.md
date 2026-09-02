# PassportOS

Firmware OS for the [FoloToy AI Passport](https://github.com/FoloToy/ai-passport):
**ESP32-C3**, 8 MB flash, **no PSRAM**, no MMU. Rust `no_std`.

The card is a **240×320 ST7789** and **three keys**. The shell is Omarchy-like on
this hardware (status bar, unified launcher, non-overlapping tiles, two
workspaces, a Settings-style menu) — not Omarchy Linux, Hyprland, or Wayland.
Apps are compiled into the image; there is no ELF/WASM loader.

## On the card

Physical keys, **top → bottom**: **UP / DOWN / OK**.

Boot is **dark**. A **48×40** badge drops in, then **PassportOS** types on
(press any key to skip). Light theme exists, but a full-panel light fill
scrambles this ST7789 (花屏), so large fills stay on proven dark field colours.

| Action | Binding |
| --- | --- |
| Home (launcher) | OK **long** (~800 ms) |
| Move / open | UP / DOWN **click**, OK **click** |
| Workspace 1 / 2 | UP / DOWN **long** — **not** while a game is focused |
| **Flap** | OK or UP **press** (not click-on-release) |
| **Stack** | OK **press** drops the sliding slab |
| **Brick** | UP / DOWN move the paddle (right side), OK **press** serves |

In Flap / Stack / Brick, UP and DOWN stay with the game: a release-click does
not hop to a neighbour tile, and holding a direction does not switch
workspaces. Long OK is still home.

**Launcher:** Pulse (ADC meter), Tap (NTAG213 facts), Flap, Stack, Brick, System.

**System:** brightness, appearance, **wifi** (scan, pick an AP, 3-key English IME
for the password; open nets skip the IME), bluetooth, sleep, keys, about.
Wi-Fi credentials stay in RAM (the 16-byte KV slot is already full).

**Status bar**, left to right: **HH:MM**, radio, workspace dots, battery %
(lightning when USB-C is supplying a host or SOC is rising). There is no
battery-backed RTC — set the clock with USB `time 14:32` (last-set plus time
since boot, stored in KV).

**NFC** is a passive **NTAG213** (144 B user). Phones talk RF; the MCU has no
bus. Tap explains that. The hardware power button is not a GPIO.

## Quick start

Needs current stable Rust (`rust-toolchain.toml` adds
`riscv32imc-unknown-none-elf`) and [`espflash`](https://github.com/esp-rs/espflash).

```bash
cargo test -p passport-core
cargo build -p passport-os --release --target riscv32imc-unknown-none-elf
espflash flash --flash-size 8mb --partition-table partitions.csv \
  --monitor target/riscv32imc-unknown-none-elf/release/passport-os
```

Or `./scripts/flash.sh` after the release build. **Do not** pass `--erase-all`:
that wipes `cardid` at `0x356000` and official Recovery at `0x700000`.

USB console is Serial/JTAG on **GPIO18/19** (macOS typically `/dev/cu.usbmodem*`).
UART0 TX is the backlight pin (**GPIO21**) — never enable UART0 as the console.

A healthy boot prints:

```
[boot] PassportOS r1
[boot] display ST7789P3 240x320 invert-on
[shell] ready workspaces=2 overlay=launcher
[ui] painted overlay=launcher
```

`bat=--` is valid when the CW2017 is absent. No reset loop. The factory image
is about **727 KB** of the 3 MB app slot.

USB lines go through the same millivolt decoder as the keys. Useful commands:
`status`, `time HH:MM`, `activate flap|stack|brick|nfc|pulse|system`,
`radio wifi|ble|off`, `key up|down|ok click|long`. Full grammar is in
[docs/APP.md](docs/APP.md).

## Tree

| Path | What |
| --- | --- |
| `crates/passport-core` | Host-testable kernel: ADC windows, compositor, shell, IME, Wi-Fi UI, games. No HAL types. |
| `firmware` | ESP32-C3 bring-up: ST7789, LEDC, ADC, I2C, I2S, USB, Wi-Fi worker, BLE, sleep |
| `docs/APP.md` | App contract (`App` + `Cx`), pin map, USB console, how to add an app |
| `AGENTS.md` | Guardrails for agents working in this repo |
| `partitions.csv` | Factory 3 MB + KV `0x350000` + protected `cardid` + Recovery |
| `scripts/flash.sh` | Flash factory slot without `--erase-all` |

Quality gate: `cargo test -p passport-core`, then the release link above.

## Hardware

Facts live in `crates/passport-core/src/board.rs`. Do not duplicate them.

| Function | Pin / bus | Notes |
| --- | --- | --- |
| ST7789P3 240×320 | SPI2 MOSI9 SCLK8 CS1 DC20, 40 MHz mode 0, invert-on | No MISO, no TE, no touch. RST tied high. |
| Backlight PWM | GPIO21 LEDC | |
| UP / DOWN / OK | GPIO0 ADC1_CH0 ladder | `{[0,150),[150,447),[447,1900)}` mV. One ADC1 only. |
| I2C0 | SDA10 SCL7 | ES8311 `0x18`, CW2017 `0x63`. One bus only. |
| I2S | MCLK6 BCLK5 WS3 DOUT2 DIN4 | 16-bit stereo 16 kHz. No 96 KB capture buffer. |
| USB Serial/JTAG | GPIO18 / GPIO19 | Console + flash. |
| Wi-Fi / BLE | on demand | Exclusive with each other and with I2S DMA. No Bluetooth Classic. |

**Not MCU-owned:** NTAG213 (phone RF only; `mcu_read` / `mcu_write` are `NoBus`)
and the hardware power button (not a GPIO).

### Flash map

| Region | Offset | Size |
| --- | --- | --- |
| factory app | `0x10000` | 3 MB |
| KV (high scores + last clock) | `0x350000` | 4 KB |
| `cardid` | `0x356000` | 16 KB |
| Recovery | `0x700000` | 1 MB |

KV is a 16-byte `POS1` slot in that 4 KB page (Flap / Stack / Brick bests and
`time`). It is not IDF NVS. Do not write `cardid` or Recovery.

## Architecture

`passport-core` is the OS: millivolt decoder, compositor, launcher, workspaces,
3-key IME, Wi-Fi picker, exclusive Wi-Fi / BLE / audio. Firmware feeds ADC
samples and USB lines into `Shell` and applies the returned side effects to
real drivers. Host tests must call those types — not a copy of the ADC windows
or paint path.

The ST7789 GRAM **is** the framebuffer. The CPU sends dirty rectangles over SPI.
A live frame must stay inside `LIVE_SPI_BUDGET` (8 KiB ≈ 1.6 ms at 40 MHz). A
full 240×298 content fill is ~29 ms: it flashes the panel and starves the button
ladder. Scene changes may fill a tile; animation ticks must not. Buttons are
sampled every **5 ms**; game sprites paint on the **20 ms** frame
(`cadence_redraw`). Do not `println` on the per-sample key path.

SRAM is tight (~72 KB heap for radio). Wi-Fi, BLE, and I2S DMA start on demand
and cannot share. The Wi-Fi worker **keeps** `WIFI` (scan then `connect_async`);
the UI task never awaits radio. `SideEffect` is `Copy`, so SSID/pass live on
`Shell`.

To ship an app: implement `passport_core::api::App`, register it, host-test
logic in `passport-core`. See [docs/APP.md](docs/APP.md).

## License

MIT. Hardware facts follow the official FoloToy AI Passport BSP pin contract.
