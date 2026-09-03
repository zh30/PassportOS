//! Live hot path: 5 ms ADC vs 20 ms sprites, shipped paint/compositor only.

use passport_core::api::{Draw, MeteredDraw, NullDraw};
use passport_core::board::Key;
use passport_core::board::{FRAME_TICK_MS, INPUT_TICK_MS, LCD_H, LCD_W};
use passport_core::boo::{BooWorld, GHOST_H, GHOST_W, MobKind};
use passport_core::boot::BootAnim;
use passport_core::brick::BrickWorld;
use passport_core::compositor::{LIVE_SPI_BUDGET, Rect, STATUS_BAR_H, rgb565_bytes, spi_time_us};
use passport_core::console::parse_line;
use passport_core::flap::{FLAP_APP_ID, FlapWorld, Redraw, cadence_redraw};
use passport_core::input::ButtonEvent;
use passport_core::paint::{FrameSig, PaintPlan};
use passport_core::shell::Shell;
use passport_core::stack::StackWorld;
use passport_core::theme::Palette;
use passport_core::tune::TuneWorld;

fn vp() -> Rect {
    Rect {
        x: 0,
        y: STATUS_BAR_H,
        w: LCD_W,
        h: LCD_H - STATUS_BAR_H,
    }
}

struct FillLog {
    clip: Rect,
    max_wh: u32,
}

impl Draw for FillLog {
    fn fill(&mut self, r: Rect, _c: u16) {
        let area = u32::from(r.w).saturating_mul(u32::from(r.h));
        if area > self.max_wh {
            self.max_wh = area;
        }
    }
    fn text(&mut self, _x: u16, _y: u16, _s: &str, _fg: u16, _bg: u16) {}
    fn text_2x(&mut self, x: u16, y: u16, s: &str, fg: u16, bg: u16) {
        self.text(x, y, s, fg, bg);
    }
    fn clip(&self) -> Rect {
        self.clip
    }
}

fn meter_live(paint: impl FnOnce(&mut MeteredDraw<'_>, &mut FillLog)) -> (u32, u32) {
    let mut n = NullDraw::new(vp());
    let mut m = MeteredDraw::new(&mut n);
    let mut fills = FillLog {
        clip: vp(),
        max_wh: 0,
    };
    paint(&mut m, &mut fills);
    (m.spi_bytes(), fills.max_wh)
}

#[test]
fn live_sprites_are_gated_off_the_adc_tick() {
    assert_eq!(INPUT_TICK_MS, 5);
    assert_eq!(FRAME_TICK_MS, 20);
    assert_eq!(cadence_redraw(Redraw::Live, false), Redraw::None);
    assert_eq!(cadence_redraw(Redraw::Live, true), Redraw::Live);
    assert_eq!(cadence_redraw(Redraw::Full, false), Redraw::Full);
    assert_eq!(cadence_redraw(Redraw::None, true), Redraw::None);
    assert!(
        spi_time_us(LIVE_SPI_BUDGET) < INPUT_TICK_MS * 1000,
        "8 KiB live SPI must fit inside one ADC sample, got {} us",
        spi_time_us(LIVE_SPI_BUDGET)
    );
}

#[test]
fn compositor_live_tick_is_sprites_not_a_playfield_wipe() {
    let mut sh = Shell::new();
    sh.register_app(FLAP_APP_ID, "flap").unwrap();
    sh.enter_home();
    sh.apply_command(parse_line("activate flap").unwrap());
    let prev = FrameSig::capture(&sh);
    sh.request_live();
    let plan = PaintPlan::diff(Some(prev), FrameSig::capture(&sh));
    assert!(plan.game_live);
    assert!(!plan.game);
    assert!(!plan.wipe_content);
    assert_eq!(plan.wipe_rows(), 0);
}

#[test]
fn shipped_live_paints_stay_inside_spi_budget() {
    let playfield = rgb565_bytes(LCD_W, LCD_H - STATUS_BAR_H);
    assert!(playfield > LIVE_SPI_BUDGET);

    let mut flap = FlapWorld::new(1);
    flap.flap();
    flap.mark_painted();
    flap.tick();
    assert_eq!(flap.redraw(), Redraw::Live);
    let (bytes, max_wh) = meter_live(|m, fills| {
        flap.paint(m, vp(), Redraw::Live, Palette::DARK);
        flap.paint(fills, vp(), Redraw::Live, Palette::DARK);
    });
    assert!(bytes > 0);
    assert!(
        bytes <= LIVE_SPI_BUDGET,
        "flap live {bytes} over {LIVE_SPI_BUDGET}"
    );
    assert!(
        max_wh < u32::from(LCD_W) * u32::from(LCD_H - STATUS_BAR_H),
        "flap live filled {max_wh} px (a 240×298 wipe)"
    );

    let mut stack = StackWorld::new(1);
    stack.drop();
    stack.mark_painted();
    stack.tick();
    assert_eq!(stack.redraw(), Redraw::Live);
    let (bytes, max_wh) = meter_live(|m, fills| {
        stack.paint(m, vp(), Redraw::Live, Palette::DARK);
        stack.paint(fills, vp(), Redraw::Live, Palette::DARK);
    });
    assert!(
        bytes <= LIVE_SPI_BUDGET,
        "stack live {bytes} over {LIVE_SPI_BUDGET}"
    );
    assert!(max_wh < u32::from(LCD_W) * u32::from(LCD_H - STATUS_BAR_H));

    let mut brick = BrickWorld::new();
    brick.input(ButtonEvent::Press(Key::Ok));
    brick.mark_painted();
    brick.tick();
    assert_eq!(brick.redraw(), Redraw::Live);
    let (bytes, max_wh) = meter_live(|m, fills| {
        brick.paint(m, vp(), Redraw::Live, Palette::DARK);
        brick.paint(fills, vp(), Redraw::Live, Palette::DARK);
    });
    assert!(
        bytes <= LIVE_SPI_BUDGET,
        "brick live {bytes} over {LIVE_SPI_BUDGET}"
    );
    assert!(max_wh < u32::from(LCD_W) * u32::from(LCD_H - STATUS_BAR_H));

    let mut boo = BooWorld::new(1);
    boo.start();
    boo.place(120, 0, MobKind::Ghost);
    boo.mark_painted();
    boo.tick();
    assert_eq!(boo.redraw(), Redraw::Live);
    let (bytes, max_wh) = meter_live(|m, fills| {
        boo.paint(m, vp(), Redraw::Live, Palette::DARK);
        boo.paint(fills, vp(), Redraw::Live, Palette::DARK);
    });
    assert!(
        bytes <= LIVE_SPI_BUDGET,
        "boo live {bytes} over {LIVE_SPI_BUDGET}"
    );
    assert!(max_wh < u32::from(LCD_W) * u32::from(LCD_H - STATUS_BAR_H));
    let full_body = rgb565_bytes(GHOST_W as u16, GHOST_H as u16).saturating_mul(2);
    assert!(
        bytes < full_body,
        "boo scroll must be strips not a full ghost erase+draw ({bytes} vs body {full_body})"
    );

    let mut tune = TuneWorld::new();
    tune.feed_hz(110);
    tune.mark_painted();
    tune.feed_hz(112);
    assert_eq!(tune.redraw(), Redraw::Live);
    let (bytes, max_wh) = meter_live(|m, fills| {
        tune.paint(m, vp(), Redraw::Live, Palette::DARK);
        tune.paint(fills, vp(), Redraw::Live, Palette::DARK);
    });
    assert!(
        bytes <= LIVE_SPI_BUDGET,
        "tune live {bytes} over {LIVE_SPI_BUDGET}"
    );
    assert!(max_wh < u32::from(LCD_W) * u32::from(LCD_H - STATUS_BAR_H));
}

#[test]
fn boot_live_frames_are_dirty_rects_not_a_band() {
    let band = rgb565_bytes(LCD_W, 80);
    let mut a = BootAnim::new();
    let mut n = NullDraw::new(Rect {
        x: 0,
        y: 0,
        w: LCD_W,
        h: LCD_H,
    });
    let mut m = MeteredDraw::new(&mut n);
    a.paint(&mut m, Palette::DARK);
    a.mark_painted();
    a.tick();
    let mut n = NullDraw::new(Rect {
        x: 0,
        y: 0,
        w: LCD_W,
        h: LCD_H,
    });
    let mut m = MeteredDraw::new(&mut n);
    let mut fills = FillLog {
        clip: Rect {
            x: 0,
            y: 0,
            w: LCD_W,
            h: LCD_H,
        },
        max_wh: 0,
    };
    a.paint(&mut m, Palette::DARK);
    a.paint(&mut fills, Palette::DARK);
    assert!(m.spi_bytes() <= LIVE_SPI_BUDGET);
    assert!(
        m.spi_bytes() < band,
        "boot live {} must not be a 240×80 band ({band})",
        m.spi_bytes()
    );
    assert!(
        fills.max_wh < u32::from(LCD_W) * 80,
        "boot live biggest rect {} is a band wipe",
        fills.max_wh
    );
}
