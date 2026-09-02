//! Host tests for the compile-in Flappy Bird world. No HAL.
//! Live paint must stay inside the compositor SPI budget — a full playfield
//! fill is what made the panel flicker and starved the OK key.

use passport_core::api::{Draw, MemoryStore, MeteredDraw, NullDraw, Store};
use passport_core::board::{Key, LCD_H, LCD_W, TYPICAL_OK_MV};
use passport_core::compositor::{rgb565_bytes, spi_time_us, Rect, LIVE_SPI_BUDGET, STATUS_BAR_H};
use passport_core::flap::{
    is_flap_input, live_budget_holds, read_best, write_best, FlapState, FlapWorld, Redraw, BIRD_H,
    BEST_KEY, COL_PIPE, COL_SKY, PIPE_W, SCROLL, WORLD_H, WORLD_W,
};
use passport_core::input::{ButtonDecoder, ButtonEvent};
use passport_core::theme::Palette;

fn vp() -> Rect {
    Rect {
        x: 0,
        y: STATUS_BAR_H,
        w: LCD_W,
        h: LCD_H - STATUS_BAR_H,
    }
}

fn spi_of(world: &FlapWorld, mode: Redraw) -> u32 {
    let mut n = NullDraw::new(vp());
    let mut m = MeteredDraw::new(&mut n);
    world.paint(&mut m, vp(), mode, Palette::DARK);
    m.spi_bytes()
}

#[test]
fn ready_flap_starts_playing() {
    let mut w = FlapWorld::new(1);
    assert_eq!(w.state, FlapState::Ready);
    w.flap();
    assert_eq!(w.state, FlapState::Playing);
    assert!(w.bird_v < 0);
}

#[test]
fn gravity_pulls_bird_down() {
    let mut w = FlapWorld::new(1);
    w.flap();
    let y0 = w.bird_y;
    for _ in 0..30 {
        w.tick();
        if w.state == FlapState::Dead {
            break;
        }
    }
    assert!(
        w.bird_y > y0 || w.state == FlapState::Dead,
        "bird_y {} after gravity",
        w.bird_y
    );
}

#[test]
fn hitting_floor_is_dead() {
    let mut w = FlapWorld::new(1);
    w.flap();
    w.bird_y = WORLD_H - BIRD_H - 2;
    w.bird_v = 8;
    for _ in 0..8 {
        w.tick();
        if w.state == FlapState::Dead {
            break;
        }
    }
    assert_eq!(w.state, FlapState::Dead);
}

#[test]
fn dead_flap_resets_to_ready() {
    let mut w = FlapWorld::new(1);
    w.flap();
    w.state = FlapState::Dead;
    w.score = 3;
    w.best = 3;
    w.flap();
    assert_eq!(w.state, FlapState::Ready);
    assert_eq!(w.score, 0);
    assert_eq!(w.best, 3);
}

#[test]
fn score_when_pipe_passes_bird() {
    let mut w = FlapWorld::new(1);
    w.flap();
    let mut scored = false;
    for _ in 0..200 {
        w.tick();
        if w.score > 0 {
            scored = true;
            break;
        }
        if w.state == FlapState::Dead {
            break;
        }
    }
    assert!(scored || w.state == FlapState::Dead);
}

#[test]
fn ready_is_full_once_then_idle() {
    let mut w = FlapWorld::new(1);
    assert_eq!(w.redraw(), Redraw::Full);
    w.mark_painted();
    assert_eq!(w.redraw(), Redraw::None);
    w.flap();
    assert_eq!(w.redraw(), Redraw::Full);
    w.mark_painted();
    assert_eq!(w.redraw(), Redraw::Live);
}

#[test]
fn playfield_fill_exceeds_live_budget() {
    assert!(live_budget_holds());
    let full = rgb565_bytes(WORLD_W as u16, WORLD_H as u16);
    assert!(full > LIVE_SPI_BUDGET * 8, "full fill {full} vs budget {LIVE_SPI_BUDGET}");
    assert!(
        spi_time_us(full) > 20_000,
        "a content wipe must not fit in the 20 ms input period, got {} us",
        spi_time_us(full)
    );
    assert!(spi_time_us(LIVE_SPI_BUDGET) < 5_000);
}

#[test]
fn full_paint_is_a_playfield_fill_live_is_not() {
    let mut w = FlapWorld::new(1);
    let full = spi_of(&w, Redraw::Full);
    assert!(
        full > LIVE_SPI_BUDGET,
        "scene paint {full} must exceed the live budget (this is the flicker)"
    );

    w.flap();
    w.mark_painted();
    w.tick();
    assert_eq!(w.redraw(), Redraw::Live);
    let live = spi_of(&w, Redraw::Live);
    assert!(live > 0, "a playing tick must push some pixels");
    assert!(
        live <= LIVE_SPI_BUDGET,
        "live paint {live} must stay inside {LIVE_SPI_BUDGET}"
    );
    assert!(
        live < full / 8,
        "live {live} must be far cheaper than scene {full}"
    );
}

#[test]
fn playing_ticks_stay_in_budget_until_death() {
    let mut w = FlapWorld::new(1);
    w.flap();
    w.mark_painted();
    for _ in 0..80 {
        if w.state != FlapState::Playing {
            break;
        }
        w.tick();
        match w.redraw() {
            Redraw::Live => {
                let b = w.live_spi_bytes();
                assert!(b <= LIVE_SPI_BUDGET, "live ops {b} over budget");
                w.mark_painted();
            }
            Redraw::Full => {
                // pipe wrap / death — scene paint is allowed to be large
                w.mark_painted();
            }
            Redraw::None => panic!("playing must request a paint"),
        }
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

fn die_with_score(w: &mut FlapWorld, score: u16) {
    w.flap();
    w.score = score;
    w.bird_y = WORLD_H;
    w.bird_v = 8;
    w.tick();
}

#[test]
fn death_card_shows_score_and_best() {
    let mut w = FlapWorld::new(1);
    die_with_score(&mut w, 7);
    assert_eq!(w.state, FlapState::Dead);
    assert_eq!(w.score, 7);
    assert_eq!(w.best, 7);
    assert!(w.is_new_best());

    let mut log = TextLog {
        clip: vp(),
        lines: heapless::Vec::new(),
    };
    w.paint(&mut log, vp(), Redraw::Full, Palette::DARK);
    let joined: heapless::String<96> = {
        let mut s = heapless::String::new();
        for line in &log.lines {
            let _ = s.push_str(line);
            let _ = s.push(' ');
        }
        s
    };
    assert!(
        joined.contains("7") && joined.contains("best") && joined.contains("new best"),
        "death card must show score and new best, got {joined:?}"
    );
    assert!(
        joined.contains("OK retry"),
        "death card must say how to retry, got {joined:?}"
    );
}

#[test]
fn best_survives_reset_via_store() {
    let mut store = MemoryStore::new();
    let mut w = FlapWorld::new(1);
    die_with_score(&mut w, 11);
    assert!(w.is_new_best());
    write_best(&mut store, w.best);
    assert_eq!(read_best(&store), 11);
    assert_eq!(store.get(BEST_KEY, &mut [0; 2]), Some(2));

    let mut w2 = FlapWorld::new(2);
    w2.seed_best(read_best(&store));
    w2.reset();
    assert_eq!(w2.state, FlapState::Ready);
    assert_eq!(w2.score, 0);
    assert_eq!(w2.best, 11);
    assert!(!w2.is_new_best());
}

#[test]
fn tying_best_is_not_a_new_best() {
    let mut store = MemoryStore::new();
    write_best(&mut store, 4);
    let mut w = FlapWorld::new(1);
    w.seed_best(read_best(&store));
    die_with_score(&mut w, 4);
    assert_eq!(w.best, 4);
    assert!(!w.is_new_best());
}

#[test]
fn ok_press_from_decoder_is_flap_input() {
    let mut d = ButtonDecoder::new();
    let mut saw_press = false;
    let mut saw_click = false;
    for _ in 0..4 {
        for ev in d.feed(TYPICAL_OK_MV, 20) {
            if is_flap_input(ev) {
                saw_press = true;
            }
            if matches!(ev, ButtonEvent::Click(Key::Ok)) {
                saw_click = true;
            }
        }
    }
    assert!(saw_press, "debounced OK down must emit Press");
    assert!(!saw_click, "click is on release; press is what flaps");
    assert!(is_flap_input(ButtonEvent::Press(Key::Ok)));
    assert!(is_flap_input(ButtonEvent::Press(Key::Up)));
    assert!(!is_flap_input(ButtonEvent::Click(Key::Ok)));
    assert!(!is_flap_input(ButtonEvent::Click(Key::Up)));
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

fn hover(w: &mut FlapWorld) {
    w.bird_y = 110;
    w.bird_v = 0;
}

#[test]
fn live_idle_does_not_punch_pipe_trailing_edge() {
    let mut w = FlapWorld::new(1);
    w.flap();
    hover(&mut w);
    w.tick();
    w.mark_painted();
    let ops = w.live_ops();
    for p in w.pipes() {
        let trail = p.x + PIPE_W - SCROLL;
        if trail < 0 {
            continue;
        }
        let tx = trail as u16;
        assert!(
            !ops.iter().any(|op| op.rgb565 == COL_SKY
                && op.rect.x == tx
                && op.rect.w == SCROLL as u16
                && op.rect.h > 20),
            "idle live paint skied the wall at x={tx}"
        );
    }
}

#[test]
fn live_scroll_only_paints_pipe_strips() {
    let mut w = FlapWorld::new(1);
    w.flap();
    for _ in 0..200 {
        hover(&mut w);
        w.tick();
        if w.state != FlapState::Playing {
            break;
        }
        if w.pipes().iter().any(|p| p.x > 0 && p.x + PIPE_W < WORLD_W) {
            break;
        }
        w.mark_painted();
    }
    w.mark_painted();
    hover(&mut w);
    w.tick();
    let mut sky_w = 0u16;
    let mut pipe_w = 0u16;
    for op in w.live_ops() {
        if op.rect.h > 20 && op.rect.w <= SCROLL as u16 + 2 {
            if op.rgb565 == COL_SKY {
                sky_w = sky_w.max(op.rect.w);
            }
            if op.rgb565 == COL_PIPE {
                pipe_w = pipe_w.max(op.rect.w);
            }
        }
    }
    assert_eq!(sky_w, SCROLL as u16);
    assert_eq!(pipe_w, SCROLL as u16);
}

#[test]
fn live_scroll_keeps_pipe_body_green() {
    let mut w = FlapWorld::new(1);
    w.flap();
    let mut on_screen = None;
    for _ in 0..200 {
        hover(&mut w);
        w.tick();
        if w.state != FlapState::Playing {
            break;
        }
        if let Some(p) = w.pipes().into_iter().find(|p| p.x > 4 && p.x + PIPE_W < WORLD_W - 4)
        {
            on_screen = Some(p);
            break;
        }
        w.mark_painted();
    }
    let p = on_screen.expect("pipe should enter the playfield");
    let mut fb = Fb::new(vp());
    w.paint(&mut fb, vp(), Redraw::Full, Palette::DARK);
    w.mark_painted();
    hover(&mut w);
    w.tick();
    w.paint(&mut fb, vp(), Redraw::Live, Palette::DARK);
    let sample_x = vp().x + (p.x + 8).max(0) as u16;
    let sample_y = vp().y + 4;
    assert_eq!(
        fb.get(sample_x, sample_y),
        COL_PIPE,
        "pipe body at ({sample_x},{sample_y}) was flashed"
    );
}

#[test]
fn live_score_plate_does_not_punch_a_pipe() {
    let mut w = FlapWorld::new(1);
    w.flap();
    let mut hit = None;
    for _ in 0..250 {
        hover(&mut w);
        w.tick();
        if w.state != FlapState::Playing {
            break;
        }
        if let Some(p) = w
            .pipes()
            .into_iter()
            .find(|p| p.x <= 12 && p.x + PIPE_W > 28 && p.gap_y > 20)
        {
            hit = Some(p);
            break;
        }
        w.mark_painted();
    }
    let p = hit.expect("a pipe should cross the score plate");
    let mut fb = Fb::new(vp());
    w.paint(&mut fb, vp(), Redraw::Full, Palette::DARK);
    w.mark_painted();
    hover(&mut w);
    w.tick();
    w.paint(&mut fb, vp(), Redraw::Live, Palette::DARK);
    let x = vp().x + (p.x + PIPE_W / 2).max(0) as u16;
    let y = vp().y + 8; // inside the old 36×8 score wipe
    assert_eq!(
        fb.get(x, y),
        COL_PIPE,
        "score plate skied the wall at ({x},{y})"
    );
    let _ = COL_SKY;
}
