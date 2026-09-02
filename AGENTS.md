# PassportOS — agent notes

- Stack: Rust `no_std` + `esp-hal` 1.2 (git tag `esp-hal-v1.2.0-rc.0`) + embassy via `esp-rtos`.
- Host-testable logic is **only** in `crates/passport-core`. Do not put HAL types there.
- ADC millivolt windows are official `{[0,150),[150,447),[447,1900)}`. Tests must call `decode_millivolts` / `ButtonDecoder::feed` / `Shell`, not a copy.
- Input: `INPUT_TICK_MS=5`, press debounce 30 ms, **release debounce 5 ms**. Sample ADC after SPI. Do not feed the decoder a copied millivolt window. Live game paint is `FRAME_TICK_MS=20` via `cadence_redraw` — never `request_live` on the 5 ms sample. No `[btn]` USB print on the ADC path. While Flap/Stack/Brick is focused, UP/DOWN click and long-press stay with the game (release-click must not hop tiles; paddle hold must not switch workspaces). Long OK is still home.
- One I2C0, one ADC1. Never construct a second bus/unit for scans.
- Wi-Fi, BLE, and large audio DMA are exclusive (`ExclusiveManager`).
- System Wi-Fi: overlay + 3-key IME live in `passport-core`. Firmware `wifi_worker` keeps `WIFI` (scan then `connect_async`). Never await radio on the UI task. `SideEffect` is `Copy`; SSID/pass stay on `Shell`.
- Flash factory app at `0x10000` (3 MB). Do not erase `cardid@0x356000` or `recovery@0x700000`. KV high-score page is `0x350000` (4 KB, between factory and cardid).
- USB console is GPIO18/19 Serial/JTAG. UART0 TX is GPIO21 (backlight) — never enable it.
- Unplugged idle standby (30 s, `IDLE_STANDBY_MS`) is `SideEffect::SetBrightness(0)` then restore; not `SleepLight` / `SleepDeep`. Wake keys are swallowed. Keep the 5 ms ADC loop while blanked; skip live SPI paints.
- Unplugged idle standby (30 s, `IDLE_STANDBY_MS`) is `SideEffect::SetBrightness(0)` then restore; not `SleepLight` / `SleepDeep`. Wake keys are swallowed. Keep the 5 ms ADC loop while blanked; skip live SPI paints.
- NTAG213 is a passive tag (no MCU bus). Firmware exposes NDEF encode + Tap app; `mcu_read`/`mcu_write` are `NoBus`. Power button is not a GPIO.
- Quality: `cargo test -p passport-core` then `cargo build -p passport-os --release --target riscv32imc-unknown-none-elf`.
