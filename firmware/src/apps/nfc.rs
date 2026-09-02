//! Tap: NTAG213 is on the card, not on the MCU bus.

use passport_core::api::{
    apply_notes, App, Battery, ClippedDraw, Cx, MemoryStore, NullAudio, NullPower, NullRadio,
};
use passport_core::compositor::Rect;
use passport_core::nfc::{encode_uri_tlv, DEFAULT_URI, NTAG213};
use passport_core::AppId;

use crate::draw::{content_rect, LcdDraw};
use crate::st7789::St7789;
use passport_core::theme::Palette;

pub const NFC_ID: AppId = AppId(3);

pub struct NfcApp;

impl NfcApp {
    pub const fn new() -> Self {
        Self
    }

    fn paint_body(&self, cx: &mut Cx<'_>, vp: Rect, p: Palette) {
        let tag = cx.nfc();
        let x = vp.x + 16;
        let mut y = vp.y + 12;
        cx.draw.text(x, y, "Tap", p.secondary, p.bg);
        y = y.saturating_add(16);
        cx.draw.text(x, y, tag.name, p.accent, p.bg);
        y = y.saturating_add(20);
        cx.draw.fill(
            Rect {
                x,
                y,
                w: vp.w.saturating_sub(32),
                h: 1,
            },
            p.separator,
        );
        y = y.saturating_add(12);
        cx.draw.text(x, y, "144B user mem", p.label, p.bg);
        y = y.saturating_add(16);
        cx.draw.text(x, y, "MCU not wired", p.secondary, p.bg);
        y = y.saturating_add(24);
        cx.draw.text(x, y, "phone reads NDEF", p.label, p.bg);
        y = y.saturating_add(16);
        cx.draw.text(x, y, "firmware cannot write", p.secondary, p.bg);
        y = y.saturating_add(24);
        cx.draw.fill(
            Rect {
                x,
                y,
                w: vp.w.saturating_sub(32),
                h: 1,
            },
            p.separator,
        );
        y = y.saturating_add(12);
        cx.draw.text(x, y, "sample URI", p.secondary, p.bg);
        y = y.saturating_add(16);
        cx.draw.text(x, y, DEFAULT_URI, p.label, p.bg);
    }
}

impl App for NfcApp {
    fn id(&self) -> AppId {
        NFC_ID
    }
    fn name(&self) -> &'static str {
        "nfc"
    }
    fn title(&self) -> &'static str {
        "Tap"
    }
    fn blurb(&self) -> &'static str {
        "NTAG213"
    }

    fn on_start(&mut self, _cx: &mut Cx<'_>) {
        esp_println::println!("[app] nfc start");
    }

    fn draw(&self, cx: &mut Cx<'_>, vp: Rect) {
        self.paint_body(cx, vp, Palette::DARK);

        let mut buf = [0u8; 64];
        if let Ok(n) = encode_uri_tlv(DEFAULT_URI, &mut buf) {
            let _ = n;
            let _ = NTAG213.mcu_wired;
        }
    }
}

pub fn paint<SPI, DC, CS, E>(lcd: &mut St7789<SPI, DC, CS>, app: &NfcApp, p: Palette)
where
    SPI: embedded_hal::spi::SpiBus<u8, Error = E>,
    DC: embedded_hal::digital::OutputPin,
    CS: embedded_hal::digital::OutputPin,
{
    let vp = content_rect();
    let mut lcd_draw = LcdDraw::new(lcd, vp);
    let mut clipped = ClippedDraw::new(&mut lcd_draw, vp);
    let mut audio = NullAudio;
    let mut store = MemoryStore::new();
    let mut radio = NullRadio;
    let mut power = NullPower::new(100);
    let mut cx = Cx::new(
        &mut clipped,
        &mut audio,
        &mut store,
        &mut radio,
        &mut power,
        0,
        Battery::unknown(),
        100,
        0,
    );
    app.paint_body(&mut cx, vp, p);
}

pub fn dispatch(app: &mut NfcApp, notes: &[passport_core::AppLifecycle], adc_mv: u16) {
    let vp = content_rect();
    let mut draw = passport_core::api::NullDraw::new(vp);
    let mut audio = NullAudio;
    let mut store = MemoryStore::new();
    let mut radio = NullRadio;
    let mut power = NullPower::new(100);
    let mut cx = Cx::new(
        &mut draw,
        &mut audio,
        &mut store,
        &mut radio,
        &mut power,
        0,
        Battery::unknown(),
        100,
        adc_mv,
    );
    apply_notes(app, notes, &mut cx);
}
