//! Host tests for Breakout. No HAL.

use passport_core::api::{Draw, MemoryStore, MeteredDraw, NullDraw, Store};
use passport_core::board::{Key, LCD_H, LCD_W};
use passport_core::brick::{
    live_budget_holds, read_best, write_best, BrickState, BrickWorld, BEST_KEY, BRICK_H, BRICK_N,
    BRICK_W, COLS, COL_BG, GAP, GRID_X, GRID_Y, PAD_H, WORLD_H, WORLD_W,
};
use passport_core::compositor::{rgb565_bytes, Rect, LIVE_SPI_BUDGET, STATUS_BAR_H};
use passport_core::flap::Redraw;
use passport_core::input::ButtonEvent;
use passport_core::theme::Palette;

fn vp() -> Rect {
    Rect {
        x: 0,
        y: STATUS_BAR_H,
        w: LCD_W,
        h: LCD_H - STATUS_BAR_H,
    }
}

struct TextLog {
    clip: Rect,
    lines: heapless::Vec<heapless::String<24>, 8>,
}

impl Draw for TextLog {
    fn fill(&mut self, _r: Rect, _c: u16) {}
    fn text(&mut self, _x: u16, _y: u16, s: &str, _fg: u16, _bg: u16) {
        let mut line = heapless::String::new();
        let _ = line.push_str(s);
        let _ = self.lines.push(line);
    }
    fn text_2x(&mut self, x: u16, y: u16, s: &str, fg: u16, bg: u16) {
        self.text(x, y, s, fg, bg);
    }
    fn clip(&self) -> Rect {
        self.clip
    }
}

#[test]
fn ready_ok_serves() {
    let mut w = BrickWorld::new();
    assert_eq!(w.state, BrickState::Ready);
    assert_eq!(w.bricks_left(), BRICK_N as u32);
    w.input(ButtonEvent::Press(Key::Ok));
    assert_eq!(w.state, BrickState::Playing);
    assert!(w.vx < 0);
}

#[test]
fn up_moves_paddle_up() {
    let mut w = BrickWorld::new();
    let y0 = w.pad_y;
    w.input(ButtonEvent::Press(Key::Up));
    w.tick();
    assert!(w.pad_y < y0);
    w.input(ButtonEvent::Release(Key::Up));
    let y1 = w.pad_y;
    w.tick();
    assert_eq!(w.pad_y, y1);
}

#[test]
fn miss_is_dead() {
    let mut w = BrickWorld::new();
    w.input(ButtonEvent::Press(Key::Ok));
    w.pad_y = 0;
    w.ball_x = WORLD_W - 2;
    w.ball_y = WORLD_H - 10;
    w.vx = 3;
    w.vy = 0;
    w.tick();
    assert_eq!(w.state, BrickState::Dead);
}

#[test]
fn brick_hit_scores() {
    let mut w = BrickWorld::new();
    w.input(ButtonEvent::Press(Key::Ok));
    w.ball_x = 8;
    w.ball_y = 36;
    w.vx = 2;
    w.vy = 0;
    let left = w.bricks_left();
    w.tick();
    assert_eq!(w.score, 1);
    assert_eq!(w.bricks_left(), left - 1);
}

#[test]
fn dead_ok_resets_keeping_best() {
    let mut w = BrickWorld::new();
    w.input(ButtonEvent::Press(Key::Ok));
    w.score = 4;
    w.best = 4;
    w.state = BrickState::Dead;
    w.input(ButtonEvent::Press(Key::Ok));
    assert_eq!(w.state, BrickState::Ready);
    assert_eq!(w.score, 0);
    assert_eq!(w.best, 4);
    assert_eq!(w.bricks_left(), BRICK_N as u32);
}

#[test]
fn best_uses_own_store_key() {
    let mut store = MemoryStore::new();
    write_best(&mut store, 12);
    assert_eq!(read_best(&store), 12);
    assert_eq!(store.get(BEST_KEY, &mut [0; 2]), Some(2));
    assert_ne!(BEST_KEY, passport_core::flap::BEST_KEY);
    assert_ne!(BEST_KEY, passport_core::stack::BEST_KEY);
}

#[test]
fn live_move_stays_in_spi_budget() {
    assert!(live_budget_holds());
    let mut w = BrickWorld::new();
    w.input(ButtonEvent::Press(Key::Ok));
    w.mark_painted();
    w.tick();
    assert_eq!(w.redraw(), Redraw::Live);
    let b = w.live_spi_bytes();
    assert!(b > 0);
    assert!(b <= LIVE_SPI_BUDGET, "live {b} over {LIVE_SPI_BUDGET}");
    let mut n = NullDraw::new(vp());
    let mut m = MeteredDraw::new(&mut n);
    w.paint(&mut m, vp(), Redraw::Live, Palette::DARK);
    assert!(m.spi_bytes() <= LIVE_SPI_BUDGET);
}

#[test]
fn death_card_shows_score() {
    let mut w = BrickWorld::new();
    w.input(ButtonEvent::Press(Key::Ok));
    w.pad_y = 0;
    w.ball_x = WORLD_W - 2;
    w.ball_y = WORLD_H - 10;
    w.vx = 3;
    w.tick();
    let mut log = TextLog {
        clip: vp(),
        lines: heapless::Vec::new(),
    };
    w.paint(&mut log, vp(), Redraw::Full, Palette::DARK);
    let mut joined = heapless::String::<96>::new();
    for line in &log.lines {
        let _ = joined.push_str(line);
        let _ = joined.push(' ');
    }
    assert!(
        joined.contains("OK retry") && joined.contains("best"),
        "got {joined:?}"
    );
    let _ = PAD_H;
}

#[test]
fn paddle_clamps_to_playfield() {
    let mut w = BrickWorld::new();
    w.pad_y = 0;
    w.input(ButtonEvent::Press(Key::Up));
    w.tick();
    assert_eq!(w.pad_y, 0);
    w.input(ButtonEvent::Release(Key::Up));
    w.pad_y = WORLD_H;
    w.input(ButtonEvent::Press(Key::Down));
    w.tick();
    assert_eq!(w.pad_y, WORLD_H - PAD_H);
}

fn brick_cell(i: usize) -> (i16, i16) {
    let c = (i % COLS) as i16;
    let r = (i / COLS) as i16;
    (
        GRID_X as i16 + c * (BRICK_W as i16 + GAP as i16),
        GRID_Y as i16 + r * (BRICK_H as i16 + GAP as i16),
    )
}

/// Pixel store of the shipped `paint` path (screen coords, same as the LCD).
struct Fb {
    clip: Rect,
    w: u16,
    h: u16,
    px: Vec<u16>,
}

impl Fb {
    fn new(clip: Rect) -> Self {
        Self {
            w: clip.w,
            h: clip.h,
            px: vec![0; clip.w as usize * clip.h as usize],
            clip,
        }
    }

    fn get(&self, x: u16, y: u16) -> u16 {
        let x = x.saturating_sub(self.clip.x);
        let y = y.saturating_sub(self.clip.y);
        if x < self.w && y < self.h {
            self.px[y as usize * self.w as usize + x as usize]
        } else {
            0
        }
    }
}

impl Draw for Fb {
    fn fill(&mut self, r: Rect, c: u16) {
        let Some(hit) = r.intersection(self.clip) else {
            return;
        };
        let x0 = hit.x.saturating_sub(self.clip.x);
        let y0 = hit.y.saturating_sub(self.clip.y);
        for dy in 0..hit.h {
            for dx in 0..hit.w {
                let x = x0.saturating_add(dx);
                let y = y0.saturating_add(dy);
                if x < self.w && y < self.h {
                    self.px[y as usize * self.w as usize + x as usize] = c;
                }
            }
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

#[test]
fn live_ball_trail_restores_alive_brick() {
    let mut w = BrickWorld::new();
    w.input(ButtonEvent::Press(Key::Ok));
    // Start overlapping brick 0 on its left edge, then step fully off to the
    // left. The 4px grid gap is smaller than the 6px ball, so we cannot leave
    // through a corridor without another collision.
    w.ball_x = 4;
    w.ball_y = GRID_Y as i16 + 2;
    w.vx = -2;
    w.vy = 0;
    w.mark_painted();
    let left = w.bricks_left();
    w.tick();
    assert_eq!(w.state, BrickState::Playing);
    assert_eq!(w.bricks_left(), left, "this step must not break the brick");
    assert_eq!(w.redraw(), Redraw::Live);

    let mut fb = Fb::new(vp());
    w.paint(&mut fb, vp(), Redraw::Full, Palette::DARK);
    w.paint(&mut fb, vp(), Redraw::Live, Palette::DARK);

    let sample_x = vp().x + GRID_X;
    let sample_y = vp().y + GRID_Y + 4;
    let px = fb.get(sample_x, sample_y);
    assert_ne!(px, COL_BG, "ball trail punched a hole in an alive brick");
    assert_ne!(px, 0, "sample must have been painted");
}

#[test]
fn refill_does_not_ramwr_the_playfield() {
    let mut w = BrickWorld::new();
    w.input(ButtonEvent::Press(Key::Ok));
    for i in 0..BRICK_N {
        let (x, y) = brick_cell(i);
        w.ball_x = x;
        w.ball_y = y;
        w.vx = 2;
        w.vy = 0;
        w.tick();
    }
    assert_eq!(w.state, BrickState::Playing);
    assert_eq!(w.bricks_left(), BRICK_N as u32, "cleared board must refill");
    assert_eq!(w.redraw(), Redraw::Full);

    let mut n = NullDraw::new(vp());
    let mut m = MeteredDraw::new(&mut n);
    w.paint(&mut m, vp(), Redraw::Full, Palette::DARK);
    let playfield = rgb565_bytes(WORLD_W as u16, WORLD_H as u16);
    assert!(
        m.spi_bytes() < playfield,
        "refill painted {} bytes, a 240×298 wipe is {playfield} and flashes 黑屏",
        m.spi_bytes()
    );
    assert!(m.spi_bytes() > 0);
}

#[test]
fn live_restore_stays_in_spi_budget() {
    let mut w = BrickWorld::new();
    w.input(ButtonEvent::Press(Key::Ok));
    w.ball_x = 4;
    w.ball_y = GRID_Y as i16 + 2;
    w.vx = -2;
    w.vy = 0;
    w.mark_painted();
    w.tick();
    let b = w.live_spi_bytes();
    assert!(b <= LIVE_SPI_BUDGET, "live restore {b} over {LIVE_SPI_BUDGET}");
    let mut n = NullDraw::new(vp());
    let mut m = MeteredDraw::new(&mut n);
    w.paint(&mut m, vp(), Redraw::Live, Palette::DARK);
    assert!(m.spi_bytes() <= LIVE_SPI_BUDGET);
}
