//! Stack. Logic and dirty-rect paint live in `passport_core::stack`.

use passport_core::api::{
    apply_notes, App, Battery, ClippedDraw, Cx, NullAudio, NullPower, NullRadio, Store,
};
use passport_core::compositor::Rect;
use passport_core::flap::Redraw;
use passport_core::input::ButtonEvent;
use passport_core::stack::{
    is_stack_input, read_best, write_best, StackState, StackWorld, STACK_APP_ID,
};
use passport_core::theme::Palette;
use passport_core::AppId;

use crate::draw::{content_rect, LcdDraw};
use crate::st7789::St7789;

pub const STACK_ID: AppId = STACK_APP_ID;

fn save_best_if_higher(app: &StackApp, cx: &mut Cx<'_>) {
    if app.world.best > read_best(cx.store) {
        write_best(cx.store, app.world.best);
    }
}

pub struct StackApp {
    world: StackWorld,
}

impl StackApp {
    pub fn new() -> Self {
        Self {
            world: StackWorld::new(0x51AC_B10C),
        }
    }

    pub fn redraw(&self) -> Redraw {
        self.world.redraw()
    }

    pub fn mark_painted(&mut self) {
        self.world.mark_painted();
    }
}

impl App for StackApp {
    fn id(&self) -> AppId {
        STACK_ID
    }
    fn name(&self) -> &'static str {
        "stack"
    }
    fn title(&self) -> &'static str {
        "Stack"
    }
    fn blurb(&self) -> &'static str {
        "drop slabs"
    }

    fn on_start(&mut self, cx: &mut Cx<'_>) {
        self.world.reset();
        self.world.seed_best(read_best(cx.store));
        esp_println::println!("[app] stack start best={}", self.world.best);
    }

    fn on_key(&mut self, _cx: &mut Cx<'_>, ev: ButtonEvent) {
        if is_stack_input(ev) {
            self.world.drop();
        }
    }

    fn on_stop(&mut self, cx: &mut Cx<'_>) {
        save_best_if_higher(self, cx);
    }

    fn on_blur(&mut self, cx: &mut Cx<'_>) {
        save_best_if_higher(self, cx);
    }

    fn on_tick(&mut self, cx: &mut Cx<'_>, _dt_ms: u32) {
        let lived = self.world.state != StackState::Dead;
        self.world.tick();
        if lived && self.world.state == StackState::Dead {
            save_best_if_higher(self, cx);
        }
    }

    fn draw(&self, cx: &mut Cx<'_>, vp: Rect) {
        self.world
            .paint(cx.draw, vp, self.world.redraw(), Palette::DARK);
    }
}

pub fn paint<SPI, DC, CS, E>(lcd: &mut St7789<SPI, DC, CS>, app: &StackApp, p: Palette, live: bool)
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
    app: &mut StackApp,
    notes: &[passport_core::AppLifecycle],
    adc_mv: u16,
    tick: bool,
    store: &mut dyn Store,
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
    );
    apply_notes(app, notes, &mut cx);
    if tick {
        app.on_tick(&mut cx, 20);
    }
}
