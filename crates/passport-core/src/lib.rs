//! Host-testable PassportOS kernel: input, compositor, launcher, apps, radio exclusion.
//!
//! No embassy/HAL types live here. Firmware feeds millivolts and console lines into
//! [`shell::Shell`] and applies the returned lifecycle events to real drivers.

#![cfg_attr(not(test), no_std)]

pub mod api;
pub mod app;
pub mod board;
pub mod boo;
pub mod boot;
pub mod brick;
pub mod clock;
pub mod compositor;
pub mod console;
pub mod es8311;
pub mod flap;
pub mod ime;
pub mod input;
pub mod keymap;
pub mod launcher;
pub mod menu;
pub mod mic;
pub mod nfc;
pub mod paint;
pub mod pitch;
pub mod radio;
pub mod shell;
pub mod stack;
pub mod status;
pub mod storage;
pub mod theme;
pub mod tune;
pub mod wifi;

pub use api::{
    ApiError, App, Audio, Battery, ClippedDraw, Cx, Draw, ExclusiveAudio, ExclusiveRadio,
    MemoryStore, MeteredDraw, NullAudio, NullDraw, NullPower, NullRadio, Power, Radio, Store,
    apply_notes,
};
pub use app::{AppId, AppLifecycle, AppRegistry, AppSlot};
pub use board::{
    BATTERY_POLL_PERIOD_TICKS, FRAME_TICK_MS, IDLE_TELEMETRY_PERIOD_TICKS, INPUT_TICK_MS, Key,
    KeyState, adc_bar_width, battery_poll_due, decode_millivolts, idle_telemetry_due,
};
pub use boo::{
    BOO_APP_ID, BooState, BooWorld, is_boo_lane_key, is_boo_roar_key, read_best as read_boo_best,
    write_best as write_boo_best,
};
pub use boot::{BOOT_TICK_MS, BootAnim};
pub use brick::{BRICK_APP_ID, BrickState, BrickWorld};
pub use clock::{Clock, TIME_KEY, parse_hm, read_tod, write_tod};
pub use compositor::{
    LIVE_SPI_BUDGET, Rect, STATUS_BAR_H, TILES_PER_WORKSPACE, TileLayout, WORKSPACE_COUNT,
    layout_tiles, rgb565_bytes, spi_time_us,
};
pub use console::{Command, ParseError, parse_line};
pub use es8311::{INIT as ES8311_INIT, adc_clocks_on, last_write as es8311_last_write};
pub use flap::{
    BEST_KEY, FLAP_APP_ID, FlapState, FlapWorld, Redraw, cadence_redraw, is_flap_input, read_best,
    write_best,
};
pub use ime::{IME_KEY_COUNT, IME_ROWS, Ime, ImeAction, ImeKey, key_at};
pub use input::{ButtonDecoder, ButtonEvent, DEBOUNCE_MS, RELEASE_DEBOUNCE_MS};
pub use launcher::{
    FONT_2X_W, ISLAND_COUNT, ISLAND_GAP, ISLAND_H, ISLAND_ICON, ISLAND_PAD, ISLAND_TEXT_STACK,
    LAUNCHER_CARD_W, LAUNCHER_ROW_H, LAUNCHER_Y, LauncherGroup, LauncherView, classify,
    display_name, grouped_inner_cols, is_lcd_ascii, island_blurb_cols, island_caption,
    island_caption_y, island_card_h, island_card_y, island_chevron_x, island_chevron_y,
    island_icon_y, island_text_end, island_text_x, island_title_y, launcher_visible,
    system_caption,
};
pub use mic::{LOUD_LEVEL, MicEvent, MicGate, ROAR_LEVEL, pcm16_le_level, pcm16_le_peak};
pub use nfc::{NTAG213, NTAG213_USER_BYTES, decode_uri_tlv, encode_uri_tlv, mcu_read, mcu_write};
pub use paint::{FrameSig, PaintPlan};
pub use pitch::{PITCH_N, PITCH_RATE, PitchBuf, amdf_hz, cents, fold_octave};
pub use radio::{ExclusiveManager, Resource};
pub use shell::{EventOutcome, IDLE_STANDBY_MS, Shell, SideEffect};
pub use stack::{STACK_APP_ID, StackState, StackWorld, is_stack_input};
pub use status::{RadioMode, StatusBar, charging_from_samples};
pub use storage::{
    ABOUT_PAGE_COUNT, ABOUT_PAGE_PRODUCT, ABOUT_PAGE_STORAGE, factory_free, factory_image_len,
    format_size, used_bar_width,
};
pub use theme::{Palette, Theme};
pub use tune::{Instrument, TUNE_APP_ID, TuneWorld, is_tune_instrument_key, is_tune_string_key};
pub use wifi::{WIFI_MAX_NETS, WIFI_VISIBLE, WifiAction, WifiNet, WifiPhase, WifiUi};
