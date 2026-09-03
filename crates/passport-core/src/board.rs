//! Single source of truth for FoloToy AI Passport pin/bus/ADC-window facts.
//! Values match `components/bsp/include/bsp_pins.h` in FoloToy/ai-passport.

use core::ops::Range;

/// Official millivolt windows: `{[0,150),[150,447),[447,1900)}`.
/// Boundaries are adjacent-window midpoints from the 0 / ~300 / ~595 / 3300 ladder.
pub const BTN_UP_MV: Range<u16> = 0..150;
pub const BTN_DOWN_MV: Range<u16> = 150..447;
pub const BTN_OK_MV: Range<u16> = 447..1900;

/// Typical millivolts from the official resistor ladder.
pub const TYPICAL_UP_MV: u16 = 0;
pub const TYPICAL_DOWN_MV: u16 = 300;
pub const TYPICAL_OK_MV: u16 = 595;
pub const TYPICAL_RELEASED_MV: u16 = 3300;

pub const LCD_W: u16 = 240;
pub const LCD_H: u16 = 320;
pub const LCD_SPI_HZ: u32 = 40_000_000;
pub const LCD_SPI_MODE: u8 = 0;
pub const LCD_INVERT: bool = true;

pub const PIN_LCD_MOSI: u8 = 9;
pub const PIN_LCD_SCLK: u8 = 8;
pub const PIN_LCD_CS: u8 = 1;
pub const PIN_LCD_DC: u8 = 20;
pub const PIN_LCD_BL: u8 = 21;
pub const PIN_BTN_ADC: u8 = 0;
pub const PIN_I2C_SDA: u8 = 10;
pub const PIN_I2C_SCL: u8 = 7;
pub const PIN_I2S_MCLK: u8 = 6;
pub const PIN_I2S_BCLK: u8 = 5;
pub const PIN_I2S_WS: u8 = 3;
pub const PIN_I2S_DOUT: u8 = 2;
pub const PIN_I2S_DIN: u8 = 4;
/// RX only has DIN; BCLK/WS are on TX. C3 `sig_loopback` stalls RX DMA (one
/// pop then silence). Keep false until pad-level clock share works.
pub const I2S_RX_LOOPBACK_TX: bool = false;
/// USB Serial/JTAG D- / D+ (reserved; do not reassign as GPIO).
pub const PIN_USB_DM: u8 = 18;
pub const PIN_USB_DP: u8 = 19;

pub const I2C_ES8311_ADDR: u8 = 0x18;
pub const I2C_CW2017_ADDR: u8 = 0x63;

pub const FLASH_SIZE: u32 = 8 * 1024 * 1024;
pub const FLASH_APP_OFFSET: u32 = 0x10000;
pub const FLASH_APP_SIZE: u32 = 0x300000;
/// 4 KB KV page in the gap after factory and before `cardid`. Not NVS, not Recovery.
pub const FLASH_KV_OFFSET: u32 = 0x350000;
pub const FLASH_KV_SIZE: u32 = 0x1000;
pub const FLASH_CARDID_OFFSET: u32 = 0x356000;
pub const FLASH_RECOVERY_OFFSET: u32 = 0x700000;

pub const BLE_ADV_NAME: &str = "PassportOS";
pub const OS_NAME: &str = "PassportOS";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Ok,
}

impl Key {
    pub const fn typical_mv(self) -> u16 {
        match self {
            Key::Up => TYPICAL_UP_MV,
            Key::Down => TYPICAL_DOWN_MV,
            Key::Ok => TYPICAL_OK_MV,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Key::Up => "up",
            Key::Down => "down",
            Key::Ok => "ok",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyState {
    Released,
    Down(Key),
}

/// Stable voltage meter width. 0 mV → 0, released ~3300 mV → `max_w`. Does not wrap.
pub fn adc_bar_width(mv: u16, max_w: u16) -> u16 {
    if max_w == 0 {
        return 0;
    }
    let mv = core::cmp::min(mv, TYPICAL_RELEASED_MV);
    ((u32::from(mv) * u32::from(max_w)) / u32::from(TYPICAL_RELEASED_MV)) as u16
}

/// ADC / decoder cadence. Stays well under press debounce so a tap is a few samples.
pub const INPUT_TICK_MS: u32 = 5;
/// UI / game tick. Several input samples land in one frame.
pub const FRAME_TICK_MS: u32 = 20;

/// 20 ms main-loop ticks. `0` means never print btn/status telemetry while idle.
pub const IDLE_TELEMETRY_PERIOD_TICKS: u32 = 0;

/// True when the idle loop may emit btn/status lines. Always false while
/// [`IDLE_TELEMETRY_PERIOD_TICKS`] is 0 (no sub-second idle spam).
pub const fn idle_telemetry_due(ticks: u32) -> bool {
    match IDLE_TELEMETRY_PERIOD_TICKS {
        0 => false,
        n => ticks % n == 0,
    }
}

/// Battery I2C poll: 250 × 20 ms = 5 s. Not a println cadence.
pub const BATTERY_POLL_PERIOD_TICKS: u32 = 250;

pub const fn battery_poll_due(ticks: u32) -> bool {
    ticks != 0 && ticks % BATTERY_POLL_PERIOD_TICKS == 0
}

/// Map a millivolt reading onto the official half-open ADC windows.
pub fn decode_millivolts(mv: u16) -> KeyState {
    if BTN_UP_MV.contains(&mv) {
        KeyState::Down(Key::Up)
    } else if BTN_DOWN_MV.contains(&mv) {
        KeyState::Down(Key::Down)
    } else if BTN_OK_MV.contains(&mv) {
        KeyState::Down(Key::Ok)
    } else {
        KeyState::Released
    }
}
