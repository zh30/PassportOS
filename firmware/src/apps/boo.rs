//! Shout to scare ghosts. Logic lives in `passport_core::boo`.

use passport_core::AppId;
use passport_core::api::{
    App, Battery, ClippedDraw, Cx, NullAudio, NullPower, NullRadio, Store, apply_notes,
};
use passport_core::boo::{
    BOO_APP_ID, BooState, BooWorld, is_boo_lane_key, is_boo_roar_key, read_best, write_best,
};
use passport_core::compositor::Rect;
use passport_core::flap::Redraw;
use passport_core::input::ButtonEvent;
use passport_core::mic::MicEvent;
use passport_core::theme::Palette;

use crate::draw::{LcdDraw, content_rect};
use crate::st7789::St7789;

pub const BOO_ID: AppId = BOO_APP_ID;

fn save_best_if_higher(app: &BooApp, cx: &mut Cx<'_>) {
    if app.world.best > read_best(cx.store) {
        write_best(cx.store, app.world.best);
    }
}

pub struct BooApp {
    world: BooWorld,
}

impl BooApp {
    pub const fn new() -> Self {
        Self {
            world: BooWorld::new(0xB00_5CA4E),
        }
    }

    pub fn redraw(&self) -> Redraw {
        self.world.redraw()
    }

    pub fn mark_painted(&mut self) {
        self.world.mark_painted();
    }
}

impl App for BooApp {
    fn id(&self) -> AppId {
        BOO_ID
    }
    fn name(&self) -> &'static str {
        "boo"
    }
    fn title(&self) -> &'static str {
        "Boo"
    }
    fn blurb(&self) -> &'static str {
        "shout scare"
    }

    fn on_start(&mut self, cx: &mut Cx<'_>) {
        self.world.reset();
        self.world.seed_best(read_best(cx.store));
        esp_println::println!("[app] boo start best={}", self.world.best);
    }

    fn on_key(&mut self, _cx: &mut Cx<'_>, ev: ButtonEvent) {
        if is_boo_roar_key(ev) {
            self.world.roar(false);
        }
        if is_boo_lane_key(ev) {
            self.world.nudge_lane(ev);
        }
    }

    fn on_mic(&mut self, _cx: &mut Cx<'_>, ev: MicEvent) {
        self.world.on_mic(ev);
    }

    fn on_stop(&mut self, cx: &mut Cx<'_>) {
        save_best_if_higher(self, cx);
    }

    fn on_blur(&mut self, cx: &mut Cx<'_>) {
        save_best_if_higher(self, cx);
    }

    fn on_tick(&mut self, cx: &mut Cx<'_>, _dt_ms: u32) {
        let lived = self.world.state != BooState::Dead;
        self.world.tick();
        if lived && self.world.state == BooState::Dead {
            save_best_if_higher(self, cx);
        }
    }

    fn draw(&self, cx: &mut Cx<'_>, vp: Rect) {
        self.world
            .paint(cx.draw, vp, self.world.redraw(), Palette::DARK);
    }
}

pub fn paint<SPI, DC, CS, E>(lcd: &mut St7789<SPI, DC, CS>, app: &BooApp, p: Palette, live: bool)
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
    app: &mut BooApp,
    notes: &[passport_core::AppLifecycle],
    adc_mv: u16,
    tick: bool,
    store: &mut dyn Store,
    mic_level: u8,
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
    apply_notes(app, notes, &mut cx);
    if tick {
        app.on_tick(&mut cx, 20);
    }
}
