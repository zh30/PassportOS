//! Unified app API: a small `App` surface, capabilities behind `Cx`.
//!
//! Apps do not see GPIO, SPI, ADC windows, or DMA. Firmware supplies adapters
//! that implement [`Draw`], [`Audio`], [`Store`], [`Radio`], and [`Power`].

use crate::app::AppId;
use crate::compositor::Rect;
use crate::input::ButtonEvent;
use crate::radio::{ExclusiveManager, Resource};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub soc: Option<u8>,
    pub mv: Option<u16>,
}

impl Battery {
    pub const fn unknown() -> Self {
        Self {
            soc: None,
            mv: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiError {
    Busy {
        owner: Option<Resource>,
    },
    Full,
    BadArg,
    /// NTAG213 is not on I2C/SPI/GPIO — MCU cannot read or write the tag.
    NoBus,
}

/// What an in-firmware app implements. Default hooks are no-ops.
pub trait App {
    fn id(&self) -> AppId;
    fn name(&self) -> &'static str;
    fn title(&self) -> &'static str {
        self.name()
    }
    fn blurb(&self) -> &'static str {
        ""
    }

    fn on_start(&mut self, _cx: &mut Cx<'_>) {}
    fn on_stop(&mut self, _cx: &mut Cx<'_>) {}
    fn on_focus(&mut self, _cx: &mut Cx<'_>) {}
    fn on_blur(&mut self, _cx: &mut Cx<'_>) {}
    fn on_key(&mut self, _cx: &mut Cx<'_>, _ev: ButtonEvent) {}
    fn on_mic(&mut self, _cx: &mut Cx<'_>, _ev: crate::mic::MicEvent) {}
    fn on_tick(&mut self, _cx: &mut Cx<'_>, _dt_ms: u32) {}
    fn draw(&self, cx: &mut Cx<'_>, viewport: Rect);
}

/// Per-frame capabilities. Built by the runtime, never by an app.
pub struct Cx<'a> {
    pub draw: &'a mut dyn Draw,
    pub audio: &'a mut dyn Audio,
    pub store: &'a mut dyn Store,
    pub radio: &'a mut dyn Radio,
    pub power: &'a mut dyn Power,
    pub now_ms: u32,
    pub battery: Battery,
    pub brightness: u8,
    adc_mv: u16,
    mic_level: u8,
    mic_hz: u16,
}

impl<'a> Cx<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        draw: &'a mut dyn Draw,
        audio: &'a mut dyn Audio,
        store: &'a mut dyn Store,
        radio: &'a mut dyn Radio,
        power: &'a mut dyn Power,
        now_ms: u32,
        battery: Battery,
        brightness: u8,
        adc_mv: u16,
        mic_level: u8,
    ) -> Self {
        Self {
            draw,
            audio,
            store,
            radio,
            power,
            now_ms,
            battery,
            brightness,
            adc_mv,
            mic_level,
            mic_hz: 0,
        }
    }

    /// Current button-ladder millivolts. Not a key API — keys arrive via [`App::on_key`].
    pub fn adc_mv(&self) -> u16 {
        self.adc_mv
    }

    pub fn set_adc_mv(&mut self, mv: u16) {
        self.adc_mv = mv;
    }

    /// Last system mic level (0..=255 RMS). Keys stay on [`App::on_key`]; shouts
    /// arrive via [`App::on_mic`].
    pub fn mic_level(&self) -> u8 {
        self.mic_level
    }

    pub fn set_mic_level(&mut self, level: u8) {
        self.mic_level = level;
    }

    /// Last detected pitch in Hz, or 0 if the buffer is silent.
    pub fn mic_hz(&self) -> u16 {
        self.mic_hz
    }

    pub fn set_mic_hz(&mut self, hz: u16) {
        self.mic_hz = hz;
    }

    /// Passive NTAG213 facts. MCU read/write is [`ApiError::NoBus`].
    pub fn nfc(&self) -> crate::nfc::NfcTag {
        crate::nfc::NTAG213
    }
}

pub trait Draw {
    fn fill(&mut self, r: Rect, rgb565: u16);
    fn text(&mut self, x: u16, y: u16, s: &str, fg: u16, bg: u16);
    fn text_2x(&mut self, x: u16, y: u16, s: &str, fg: u16, bg: u16);
    fn clip(&self) -> Rect;
}

pub trait Audio {
    fn play_pcm16(&mut self, rate_hz: u16) -> Result<(), ApiError>;
    fn push(&mut self, frames: &[i16]) -> usize;
    fn rec_pcm16(&mut self, rate_hz: u16) -> Result<(), ApiError>;
    fn pop(&mut self, dst: &mut [i16]) -> usize;
    fn stop(&mut self);
}

pub trait Store {
    fn get(&self, key: &[u8], dst: &mut [u8]) -> Option<usize>;
    fn put(&mut self, key: &[u8], val: &[u8]) -> Result<(), ApiError>;
}

pub trait Radio {
    fn wifi_scan(&mut self) -> Result<(), ApiError>;
    fn ble_advertise(&mut self, name: &str) -> Result<(), ApiError>;
    fn off(&mut self);
}

pub trait Power {
    fn set_brightness(&mut self, pct: u8);
    fn sleep_light(&mut self, ms: u32);
    fn sleep_deep(&mut self, ms: u32);
}

/// Intersects every draw with `clip` so apps cannot paint the status bar or
/// a neighbour tile even if they pass a wild rect.
pub struct ClippedDraw<'a> {
    inner: &'a mut dyn Draw,
    clip: Rect,
}

impl<'a> ClippedDraw<'a> {
    pub fn new(inner: &'a mut dyn Draw, clip: Rect) -> Self {
        Self { inner, clip }
    }
}

impl Draw for ClippedDraw<'_> {
    fn fill(&mut self, r: Rect, rgb565: u16) {
        if let Some(hit) = r.intersection(self.clip) {
            self.inner.fill(hit, rgb565);
        }
    }

    fn text(&mut self, x: u16, y: u16, s: &str, fg: u16, bg: u16) {
        if self.clip.contains(x, y) {
            self.inner.text(x, y, s, fg, bg);
        }
    }

    fn text_2x(&mut self, x: u16, y: u16, s: &str, fg: u16, bg: u16) {
        if self.clip.contains(x, y) {
            self.inner.text_2x(x, y, s, fg, bg);
        }
    }

    fn clip(&self) -> Rect {
        self.clip
    }
}

/// Audio adapter that refuses to share SRAM with Wi-Fi/BLE.
pub struct ExclusiveAudio<'a> {
    exclusive: &'a mut ExclusiveManager,
    playing: bool,
}

impl<'a> ExclusiveAudio<'a> {
    pub fn new(exclusive: &'a mut ExclusiveManager) -> Self {
        Self {
            exclusive,
            playing: false,
        }
    }

    pub fn playing(&self) -> bool {
        self.playing
    }
}

impl Audio for ExclusiveAudio<'_> {
    fn play_pcm16(&mut self, _rate_hz: u16) -> Result<(), ApiError> {
        if !self.exclusive.can_share_with(Resource::Audio) {
            return Err(ApiError::Busy {
                owner: self.exclusive.owner(),
            });
        }
        self.exclusive.acquire(Resource::Audio);
        self.playing = true;
        Ok(())
    }

    fn push(&mut self, frames: &[i16]) -> usize {
        if self.playing { frames.len() } else { 0 }
    }

    fn rec_pcm16(&mut self, _rate_hz: u16) -> Result<(), ApiError> {
        if !self.exclusive.can_share_with(Resource::Audio) {
            return Err(ApiError::Busy {
                owner: self.exclusive.owner(),
            });
        }
        self.exclusive.acquire(Resource::Audio);
        self.playing = true;
        Ok(())
    }

    fn pop(&mut self, _dst: &mut [i16]) -> usize {
        0
    }

    fn stop(&mut self) {
        if self.playing {
            self.exclusive.release(Resource::Audio);
            self.playing = false;
        }
    }
}

/// Radio adapter: Wi-Fi and BLE are exclusive with each other and with audio.
pub struct ExclusiveRadio<'a> {
    exclusive: &'a mut ExclusiveManager,
}

impl<'a> ExclusiveRadio<'a> {
    pub fn new(exclusive: &'a mut ExclusiveManager) -> Self {
        Self { exclusive }
    }
}

impl Radio for ExclusiveRadio<'_> {
    fn wifi_scan(&mut self) -> Result<(), ApiError> {
        if !self.exclusive.can_share_with(Resource::Wifi) {
            return Err(ApiError::Busy {
                owner: self.exclusive.owner(),
            });
        }
        self.exclusive.acquire(Resource::Wifi);
        Ok(())
    }

    fn ble_advertise(&mut self, name: &str) -> Result<(), ApiError> {
        if name.is_empty() {
            return Err(ApiError::BadArg);
        }
        if !self.exclusive.can_share_with(Resource::Ble) {
            return Err(ApiError::Busy {
                owner: self.exclusive.owner(),
            });
        }
        self.exclusive.acquire(Resource::Ble);
        Ok(())
    }

    fn off(&mut self) {
        if matches!(self.exclusive.owner(), Some(Resource::Wifi | Resource::Ble)) {
            let owner = self.exclusive.owner().unwrap();
            self.exclusive.release(owner);
        }
    }
}

const STORE_KEY: usize = 16;
const STORE_VAL: usize = 64;
const STORE_N: usize = 8;

/// RAM store used on host and as the firmware fallback before NVS is wired.
pub struct MemoryStore {
    entries: heapless::Vec<(heapless::Vec<u8, STORE_KEY>, heapless::Vec<u8, STORE_VAL>), STORE_N>,
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStore {
    pub const fn new() -> Self {
        Self {
            entries: heapless::Vec::new(),
        }
    }
}

impl Store for MemoryStore {
    fn get(&self, key: &[u8], dst: &mut [u8]) -> Option<usize> {
        let hit = self.entries.iter().find(|(k, _)| k.as_slice() == key)?;
        let n = core::cmp::min(dst.len(), hit.1.len());
        dst[..n].copy_from_slice(&hit.1[..n]);
        Some(n)
    }

    fn put(&mut self, key: &[u8], val: &[u8]) -> Result<(), ApiError> {
        if key.is_empty() || key.len() > STORE_KEY || val.len() > STORE_VAL {
            return Err(ApiError::BadArg);
        }
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| k.as_slice() == key) {
            slot.1.clear();
            let _ = slot.1.extend_from_slice(val);
            return Ok(());
        }
        let mut k = heapless::Vec::new();
        k.extend_from_slice(key).map_err(|_| ApiError::BadArg)?;
        let mut v = heapless::Vec::new();
        v.extend_from_slice(val).map_err(|_| ApiError::BadArg)?;
        self.entries.push((k, v)).map_err(|_| ApiError::Full)
    }
}

pub struct NullDraw {
    clip: Rect,
}

impl NullDraw {
    pub fn new(clip: Rect) -> Self {
        Self { clip }
    }
}

impl Draw for NullDraw {
    fn fill(&mut self, _r: Rect, _c: u16) {}
    fn text(&mut self, _x: u16, _y: u16, _s: &str, _fg: u16, _bg: u16) {}
    fn text_2x(&mut self, _x: u16, _y: u16, _s: &str, _fg: u16, _bg: u16) {}
    fn clip(&self) -> Rect {
        self.clip
    }
}

/// Counts RGB565 pixels pushed through [`Draw`]. Host tests lock the live SPI budget.
pub struct MeteredDraw<'a> {
    inner: &'a mut dyn Draw,
    pub pixels: u32,
}

impl<'a> MeteredDraw<'a> {
    pub fn new(inner: &'a mut dyn Draw) -> Self {
        Self { inner, pixels: 0 }
    }

    pub fn spi_bytes(&self) -> u32 {
        self.pixels.saturating_mul(2)
    }
}

impl Draw for MeteredDraw<'_> {
    fn fill(&mut self, r: Rect, rgb565: u16) {
        self.pixels = self
            .pixels
            .saturating_add(u32::from(r.w).saturating_mul(u32::from(r.h)));
        self.inner.fill(r, rgb565);
    }

    fn text(&mut self, x: u16, y: u16, s: &str, fg: u16, bg: u16) {
        // 6×8 glyphs, same as the firmware font.
        let n = s.len() as u32;
        self.pixels = self.pixels.saturating_add(n.saturating_mul(6 * 8));
        self.inner.text(x, y, s, fg, bg);
    }

    fn text_2x(&mut self, x: u16, y: u16, s: &str, fg: u16, bg: u16) {
        let n = s.len() as u32;
        self.pixels = self.pixels.saturating_add(n.saturating_mul(12 * 16));
        self.inner.text_2x(x, y, s, fg, bg);
    }

    fn clip(&self) -> Rect {
        self.inner.clip()
    }
}

pub struct NullAudio;

impl Audio for NullAudio {
    fn play_pcm16(&mut self, _rate_hz: u16) -> Result<(), ApiError> {
        Ok(())
    }
    fn push(&mut self, frames: &[i16]) -> usize {
        frames.len()
    }
    fn rec_pcm16(&mut self, _rate_hz: u16) -> Result<(), ApiError> {
        Ok(())
    }
    fn pop(&mut self, _dst: &mut [i16]) -> usize {
        0
    }
    fn stop(&mut self) {}
}

pub struct NullRadio;

impl Radio for NullRadio {
    fn wifi_scan(&mut self) -> Result<(), ApiError> {
        Ok(())
    }
    fn ble_advertise(&mut self, _name: &str) -> Result<(), ApiError> {
        Ok(())
    }
    fn off(&mut self) {}
}

pub struct NullPower {
    pub brightness: u8,
}

impl NullPower {
    pub fn new(brightness: u8) -> Self {
        Self { brightness }
    }
}

impl Power for NullPower {
    fn set_brightness(&mut self, pct: u8) {
        self.brightness = pct.min(100);
    }
    fn sleep_light(&mut self, _ms: u32) {}
    fn sleep_deep(&mut self, _ms: u32) {}
}

/// Dispatch shell lifecycle notes onto one app. Firmware iterates installed apps.
pub fn apply_notes(app: &mut dyn App, notes: &[crate::app::AppLifecycle], cx: &mut Cx<'_>) {
    use crate::app::AppLifecycle;
    for n in notes {
        match *n {
            AppLifecycle::Start(id) if id == app.id() => app.on_start(cx),
            AppLifecycle::Stop(id) if id == app.id() => app.on_stop(cx),
            AppLifecycle::Focus(id) if id == app.id() => app.on_focus(cx),
            AppLifecycle::Blur(id) if id == app.id() => app.on_blur(cx),
            AppLifecycle::Input(id, ev) if id == app.id() => app.on_key(cx, ev),
            AppLifecycle::Mic(id, ev) if id == app.id() => app.on_mic(cx, ev),
            _ => {}
        }
    }
}
