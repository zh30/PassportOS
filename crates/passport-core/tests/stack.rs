//! Host tests for Stack. No HAL.

use passport_core::api::{Draw, MemoryStore, MeteredDraw, NullDraw, Store};
use passport_core::board::{LCD_H, LCD_W};
use passport_core::compositor::{Rect, LIVE_SPI_BUDGET, STATUS_BAR_H};
use passport_core::flap::Redraw;
use passport_core::input::ButtonEvent;
use passport_core::stack::{
    is_stack_input, live_budget_holds, read_best, write_best, StackState, StackWorld, BASE_W,
    BEST_KEY, COL_BG, COL_MOVE, MIN_W, START_SPEED, WORLD_W,
};
use passport_core::theme::Palette;
use passport_core::board::Key;

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
fn ready_drop_starts_playing() {
    let mut w = StackWorld::new(1);
    assert_eq!(w.state, StackState::Ready);
    assert_eq!(w.slabs().len(), 1);
    w.drop();
    assert_eq!(w.state, StackState::Playing);
    assert_eq!(w.mover.w, BASE_W);
}

#[test]
fn tick_moves_and_bounces() {
    let mut w = StackWorld::new(1);
    w.drop();
    let x0 = w.mover.x;
    w.mark_painted();
    w.tick();
    assert_ne!(w.mover.x, x0);
    w.mover.x = WORLD_W;
    w.tick();
    assert!(w.mover.x + w.mover.w <= WORLD_W);
}

#[test]
fn miss_is_dead() {
    let mut w = StackWorld::new(1);
    w.drop();
    w.mover.x = WORLD_W - MIN_W;
    w.mover.w = MIN_W;
    w.drop();
    assert_eq!(w.state, StackState::Dead);
}

#[test]
fn overlap_places_and_scores() {
    let mut w = StackWorld::new(1);
    w.drop();
    w.mover.x = w.slabs()[0].x;
    w.mover.w = w.slabs()[0].w;
    w.drop();
    assert_eq!(w.state, StackState::Playing);
    assert_eq!(w.score, 1);
    assert_eq!(w.slabs().len(), 2);
}

#[test]
fn dead_drop_resets_keeping_best() {
    let mut w = StackWorld::new(1);
    w.drop();
    w.mover.x = w.slabs()[0].x;
    w.drop();
    w.best = w.score.max(3);
    w.state = StackState::Dead;
    let best = w.best;
    w.drop();
    assert_eq!(w.state, StackState::Ready);
    assert_eq!(w.score, 0);
    assert_eq!(w.best, best);
}

#[test]
fn best_uses_own_store_key() {
    let mut store = MemoryStore::new();
    write_best(&mut store, 9);
    assert_eq!(read_best(&store), 9);
    assert_eq!(store.get(BEST_KEY, &mut [0; 2]), Some(2));
    assert_ne!(BEST_KEY, passport_core::flap::BEST_KEY);
}

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
fn live_slide_does_not_repaint_the_overlap() {
    let mut w = StackWorld::new(1);
    w.drop();
    w.mark_painted();
    w.tick();
    assert_eq!(w.redraw(), Redraw::Live);
    let ops = w.live_ops();
    assert_eq!(ops.len(), 2, "vacated strip + leading strip");
    assert_eq!(ops[0].1, COL_BG);
    assert_eq!(ops[1].1, COL_MOVE);
    assert_eq!(
        ops[0].0.w,
        START_SPEED as u16,
        "must not refill the whole slab ({})",
        ops[0].0.w
    );
    assert_eq!(ops[1].0.w, START_SPEED as u16);
    assert!(
        ops[0].0.w < BASE_W as u16,
        "overlap blinks if the whole {}-wide slab is erased",
        BASE_W
    );
}

#[test]
fn live_slide_keeps_overlap_yellow() {
    let mut w = StackWorld::new(1);
    w.drop();
    let mut fb = Fb::new(vp());
    w.paint(&mut fb, vp(), Redraw::Full, Palette::DARK);
    w.mark_painted();
    let x0 = w.mover.x;
    w.tick();
    w.paint(&mut fb, vp(), Redraw::Live, Palette::DARK);
    let y = vp().y + (w.mover_y() as u16) + 4;
    // Pixel that stayed under the slab must still be yellow, not COL_BG.
    let stay_x = vp().x + (x0 + START_SPEED + 4).max(0) as u16;
    assert_eq!(fb.get(stay_x, y), COL_MOVE, "overlap was flashed to background");
    let vacated_x = vp().x + x0.max(0) as u16;
    assert_eq!(fb.get(vacated_x, y), COL_BG);
    let lead_x = vp().x + (w.mover.x + w.mover.w - 1).max(0) as u16;
    assert_eq!(fb.get(lead_x, y), COL_MOVE);
}

#[test]
fn live_without_move_is_a_nop() {
    let mut w = StackWorld::new(1);
    w.drop();
    w.mark_painted();
    assert!(w.live_ops().is_empty());
    let mut n = NullDraw::new(vp());
    let mut m = MeteredDraw::new(&mut n);
    w.paint(&mut m, vp(), Redraw::Live, Palette::DARK);
    assert_eq!(m.spi_bytes(), 0);
}

#[test]
fn live_move_stays_in_spi_budget() {
    assert!(live_budget_holds());
    let mut w = StackWorld::new(1);
    w.drop();
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
    let mut w = StackWorld::new(1);
    w.drop();
    w.mover.x = WORLD_W - MIN_W;
    w.mover.w = MIN_W;
    w.drop();
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
}

#[test]
fn ok_press_is_stack_input() {
    assert!(is_stack_input(ButtonEvent::Press(Key::Ok)));
    assert!(!is_stack_input(ButtonEvent::Press(Key::Up)));
    assert!(!is_stack_input(ButtonEvent::Click(Key::Ok)));
}
