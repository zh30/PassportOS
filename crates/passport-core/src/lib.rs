//! Host-testable PassportOS kernel: input, compositor, launcher, apps, radio exclusion.
//!
//! No embassy/HAL types live here. Firmware feeds millivolts and console lines into
//! [`shell::Shell`] and applies the returned lifecycle events to real drivers.

#![cfg_attr(not(test), no_std)]

pub mod api;
pub mod app;
pub mod board;
pub mod boot;
pub mod clock;
pub mod compositor;
pub mod flap;
pub mod stack;
pub mod brick;
pub mod console;
pub mod ime;
pub mod wifi;
pub mod input;
pub mod keymap;
pub mod launcher;
pub mod menu;
pub mod nfc;
pub mod paint;
pub mod radio;
pub mod shell;
pub mod status;
pub mod theme;

pub use api::{
    apply_notes, ApiError, App, Audio, Battery, ClippedDraw, Cx, Draw, ExclusiveAudio,
    ExclusiveRadio, MemoryStore, MeteredDraw, NullAudio, NullDraw, NullPower, NullRadio, Power,
    Radio, Store,
};
pub use app::{AppId, AppLifecycle, AppRegistry, AppSlot};
pub use boot::{BootAnim, BOOT_TICK_MS};
pub use clock::{parse_hm, read_tod, write_tod, Clock, TIME_KEY};
pub use board::{
    Key, KeyState, adc_bar_width, battery_poll_due, decode_millivolts, idle_telemetry_due,
    BATTERY_POLL_PERIOD_TICKS, FRAME_TICK_MS, IDLE_TELEMETRY_PERIOD_TICKS, INPUT_TICK_MS,
};
pub use input::{ButtonDecoder, ButtonEvent, DEBOUNCE_MS, RELEASE_DEBOUNCE_MS};
pub use compositor::{
    layout_tiles, rgb565_bytes, spi_time_us, Rect, TileLayout, LIVE_SPI_BUDGET, STATUS_BAR_H,
    TILES_PER_WORKSPACE, WORKSPACE_COUNT,
};
pub use flap::{
    cadence_redraw, is_flap_input, read_best, write_best, FlapState, FlapWorld, Redraw, BEST_KEY,
    FLAP_APP_ID,
};
pub use stack::{is_stack_input, StackState, StackWorld, STACK_APP_ID};
pub use brick::{BrickState, BrickWorld, BRICK_APP_ID};
pub use console::{Command, ParseError, parse_line};
pub use ime::{key_at, Ime, ImeAction, ImeKey, IME_KEY_COUNT, IME_ROWS};
pub use wifi::{WifiAction, WifiNet, WifiPhase, WifiUi, WIFI_MAX_NETS, WIFI_VISIBLE};
pub use nfc::{decode_uri_tlv, encode_uri_tlv, mcu_read, mcu_write, NTAG213, NTAG213_USER_BYTES};
pub use paint::{FrameSig, PaintPlan};
pub use radio::{ExclusiveManager, Resource};
pub use shell::{EventOutcome, Shell, SideEffect};
pub use status::{charging_from_samples, RadioMode, StatusBar};
pub use theme::{Palette, Theme};
