//! Sample app: live ADC millivolt meter.

use core::fmt::Write as _;

use passport_core::api::{
    apply_notes, App, Battery, ClippedDraw, Cx, MemoryStore, NullAudio, NullPower, NullRadio,
};
use passport_core::board::{adc_bar_width, decode_millivolts, KeyState, TYPICAL_RELEASED_MV};
use passport_core::compositor::Rect;
use passport_core::AppId;

use crate::draw::{content_rect, LcdDraw};
use crate::st7789::{St7789, WIDTH};
use passport_core::theme::Palette;

pub const PULSE_ID: AppId = AppId(1);

pub struct PulseApp {
    last_mv: u16,
}

impl PulseApp {
    pub const fn new() -> Self {
        Self {
            last_mv: TYPICAL_RELEASED_MV,
        }
    }

    pub fn last_mv(&self) -> u16 {
        self.last_mv
    }

    pub fn draw_meter(&self, cx: &mut Cx<'_>, vp: Rect, p: Palette) {
        let x = vp.x + 16;
        let y = vp.y + 8;
        let key = match decode_millivolts(self.last_mv) {
            KeyState::Released => "released",
            KeyState::Down(k) => k.as_str(),
        };
        let mut num = heapless::String::<16>::new();
        let _ = write!(num, "{} mV", self.last_mv);

        cx.draw.fill(
            Rect {
                x,
                y: y + 26,
                w: WIDTH.saturating_sub(40),
                h: 58,
            },
            p.bg,
        );
        cx.draw.text(x, y + 28, num.as_str(), p.label, p.bg);
        cx.draw.text(x, y + 48, key, p.secondary, p.bg);

        let track = WIDTH.saturating_sub(40);
        cx.draw.fill(
            Rect {
                x,
                y: y + 72,
                w: track,
                h: 4,
            },
            p.separator,
        );
        let bar_w = adc_bar_width(self.last_mv, track);
        if bar_w > 0 {
            cx.draw.fill(
                Rect {
                    x,
                    y: y + 70,
                    w: bar_w,
                    h: 8,
                },
                p.accent,
            );
        }
    }
}

impl App for PulseApp {
    fn id(&self) -> AppId {
        PULSE_ID
    }
    fn name(&self) -> &'static str {
        "pulse"
    }
    fn title(&self) -> &'static str {
        "Pulse"
    }
    fn blurb(&self) -> &'static str {
        "live meter"
    }

    fn on_start(&mut self, _cx: &mut Cx<'_>) {
        esp_println::println!("[app] pulse start");
    }

    fn on_tick(&mut self, cx: &mut Cx<'_>, _dt_ms: u32) {
        self.last_mv = cx.adc_mv();
    }

    fn draw(&self, cx: &mut Cx<'_>, vp: Rect) {
        let p = Palette::DARK;
        let x = vp.x + 16;
        let y = vp.y + 8;
        cx.draw.text(x, y, "Pulse", p.secondary, p.bg);
        self.draw_meter(cx, vp, p);
        cx.draw.text(x, y + 92, "0            3.3V", p.secondary, p.bg);
    }
}

/// Draw Pulse through the App API (clipped to the content tile).
pub fn paint<SPI, DC, CS, E>(lcd: &mut St7789<SPI, DC, CS>, app: &PulseApp, p: Palette)
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
        app.last_mv(),
    );
    let x = vp.x + 16;
    let y = vp.y + 8;
    cx.draw.text(x, y, "Pulse", p.secondary, p.bg);
    app.draw_meter(&mut cx, vp, p);
    cx.draw
        .text(x, y + 92, "0            3.3V", p.secondary, p.bg);
}

pub fn paint_meter<SPI, DC, CS, E>(lcd: &mut St7789<SPI, DC, CS>, app: &PulseApp, p: Palette)
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
        app.last_mv(),
    );
    app.draw_meter(&mut cx, vp, p);
}

pub fn dispatch(app: &mut PulseApp, notes: &[passport_core::AppLifecycle], adc_mv: u16) {
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
    app.on_tick(&mut cx, 20);
}
