//! Guitar / ukulele / violin tuner. Logic lives in `passport_core::tune`.

use passport_core::AppId;
use passport_core::api::{
    App, Battery, ClippedDraw, Cx, NullAudio, NullPower, NullRadio, Store, apply_notes,
};
use passport_core::compositor::Rect;
use passport_core::flap::Redraw;
use passport_core::input::ButtonEvent;
use passport_core::pitch::{PITCH_N, PitchBuf};
use passport_core::theme::Palette;
use passport_core::tune::{TUNE_APP_ID, TuneWorld, is_tune_instrument_key, is_tune_string_key};

use crate::draw::{LcdDraw, content_rect};
use crate::st7789::St7789;

pub const TUNE_ID: AppId = TUNE_APP_ID;

pub struct TuneApp {
    world: TuneWorld,
}

impl TuneApp {
    pub const fn new() -> Self {
        Self {
            world: TuneWorld::new(),
        }
    }

    pub fn redraw(&self) -> Redraw {
        self.world.redraw()
    }

    pub fn mark_painted(&mut self) {
        self.world.mark_painted();
    }
}

impl App for TuneApp {
    fn id(&self) -> AppId {
        TUNE_ID
    }
    fn name(&self) -> &'static str {
        "tune"
    }
    fn title(&self) -> &'static str {
        "Tune"
    }
    fn blurb(&self) -> &'static str {
        "guitar uke violin"
    }

    fn on_start(&mut self, _cx: &mut Cx<'_>) {
        self.world = TuneWorld::new();
        esp_println::println!("[app] tune start");
    }

    fn on_key(&mut self, _cx: &mut Cx<'_>, ev: ButtonEvent) {
        if is_tune_instrument_key(ev) {
            self.world.cycle_instrument();
        }
        if is_tune_string_key(ev) {
            self.world.nudge_string(ev);
        }
    }

    fn on_tick(&mut self, cx: &mut Cx<'_>, _dt_ms: u32) {
        if cx.mic_hz() > 0 {
            self.world.feed_hz(cx.mic_hz());
        }
    }

    fn draw(&self, cx: &mut Cx<'_>, vp: Rect) {
        self.world
            .paint(cx.draw, vp, self.world.redraw(), Palette::DARK);
    }
}

pub fn paint<SPI, DC, CS, E>(lcd: &mut St7789<SPI, DC, CS>, app: &TuneApp, p: Palette, live: bool)
where
    SPI: embedded_hal::spi::SpiBus<u8, Error = E>,
    DC: embedded_hal::digital::OutputPin,
    CS: embedded_hal::digital::OutputPin,
{
    let vp = content_rect();
    let mut lcd_draw = LcdDraw::new(lcd, vp);
    let mut clipped = ClippedDraw::new(&mut lcd_draw, vp);
    let mode = if live { Redraw::Live } else { Redraw::Full };
    app.world.paint(&mut clipped, vp, mode, p);
}

pub fn dispatch(
    app: &mut TuneApp,
    notes: &[passport_core::AppLifecycle],
    adc_mv: u16,
    tick: bool,
    store: &mut dyn Store,
    mic_level: u8,
    mic_hz: u16,
    pitch: Option<&PitchBuf>,
) {
    let vp = content_rect();
    let mut draw = passport_core::api::NullDraw::new(vp);
    let mut audio = NullAudio;
    let mut radio = NullRadio;
    let mut power = NullPower::new(100);
    let mut cx = Cx::new(
        &mut draw,
        &mut audio,
        store,
        &mut radio,
        &mut power,
        0,
        Battery::unknown(),
        100,
        adc_mv,
        mic_level,
    );
    cx.set_mic_hz(mic_hz);
    apply_notes(app, notes, &mut cx);
    if tick {
        if let Some(buf) = pitch {
            if buf.len() >= 64 {
                let mut tmp = [0i16; PITCH_N];
                let n = buf.copy_linear(&mut tmp);
                app.world.listen(&tmp[..n]);
            }
        } else {
            app.on_tick(&mut cx, 20);
        }
    }
}
