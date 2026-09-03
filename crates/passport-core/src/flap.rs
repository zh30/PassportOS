//! Minimal Flappy Bird: gravity, one-button flap, two scrolling pipes.
//!
//! Paint is dirty rectangles. A live tick never refills the playfield — that
//! 240×298 SPI fill is ~29 ms and both flashes the ST7789 and starves ADC.

use core::fmt::Write as _;

use heapless::Vec;

use crate::api::{Draw, Store};
use crate::app::AppId;
use crate::board::Key;
use crate::compositor::{LIVE_SPI_BUDGET, Rect, rgb565_bytes};
use crate::input::ButtonEvent;
use crate::theme::Palette;

pub const FLAP_APP_ID: AppId = AppId(4);

pub const WORLD_W: i16 = 240;
pub const WORLD_H: i16 = 298;
pub const BIRD_X: i16 = 40;
pub const BIRD_W: i16 = 12;
pub const BIRD_H: i16 = 10;
pub const PIPE_W: i16 = 22;
pub const GAP: i16 = 76;
pub const GROUND: i16 = 12;
pub const SCROLL: i16 = 2;
pub const GRAVITY: i16 = 1;
pub const FLAP_V: i16 = -7;
pub const PIPE_SPACE: i16 = 118;

pub const COL_SKY: u16 = 0x10A2;
pub const COL_BIRD: u16 = 0xFE60;
pub const COL_EYE: u16 = 0x18A2;
pub const COL_PIPE: u16 = 0x07E0;
pub const COL_GROUND: u16 = 0x2945;

/// Store key for the persisted high score. Two little-endian bytes.
pub const BEST_KEY: &[u8] = b"best";

const CARD_X: u16 = 16;
const CARD_W: u16 = 208;
const CARD_Y: u16 = 86;
const CARD_H: u16 = 124;
const FONT_W: u16 = 6;
const FONT_2X: u16 = 12;

const SCORE_X: i16 = 8;
const SCORE_Y: i16 = 6;
const SCORE_W: i16 = 36;
const SCORE_H: i16 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlapState {
    Ready,
    Playing,
    Dead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Redraw {
    None,
    Full,
    Live,
}

/// Live sprites belong on [`crate::board::FRAME_TICK_MS`]. Scene changes
/// (`Full`) paint immediately so a death card is not delayed a frame.
/// The 5 ms ADC loop must pass `frame_due=false`.
pub const fn cadence_redraw(want: Redraw, frame_due: bool) -> Redraw {
    match want {
        Redraw::None => Redraw::None,
        Redraw::Full => Redraw::Full,
        Redraw::Live => {
            if frame_due {
                Redraw::Live
            } else {
                Redraw::None
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pipe {
    pub x: i16,
    pub gap_y: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FillOp {
    pub rect: Rect,
    pub rgb565: u16,
}

#[derive(Clone, Debug)]
pub struct FlapWorld {
    pub state: FlapState,
    pub bird_y: i16,
    pub bird_v: i16,
    pub score: u16,
    pub best: u16,
    pipes: [Pipe; 2],
    rng: u32,
    scored: [bool; 2],
    prev_bird_y: i16,
    prev_pipes: [Pipe; 2],
    prev_score: u16,
    scene_dirty: bool,
    committed: bool,
    best_at_run_start: u16,
    new_best: bool,
}

impl Default for FlapWorld {
    fn default() -> Self {
        Self::new(0xA5A5_5A5A)
    }
}

impl FlapWorld {
    pub const fn new(seed: u32) -> Self {
        let pipes = [
            Pipe {
                x: WORLD_W + 20,
                gap_y: 90,
            },
            Pipe {
                x: WORLD_W + 20 + PIPE_SPACE,
                gap_y: 130,
            },
        ];
        let bird_y = WORLD_H / 2;
        Self {
            state: FlapState::Ready,
            bird_y,
            bird_v: 0,
            score: 0,
            best: 0,
            pipes,
            rng: seed | 1,
            scored: [false, false],
            prev_bird_y: bird_y,
            prev_pipes: pipes,
            prev_score: 0,
            scene_dirty: true,
            committed: false,
            best_at_run_start: 0,
            new_best: false,
        }
    }

    pub fn seed_best(&mut self, n: u16) {
        if n > self.best {
            self.best = n;
        }
    }

    pub fn is_new_best(&self) -> bool {
        self.new_best
    }

    pub fn reset(&mut self) {
        let best = self.best;
        let rng = self.rng;
        *self = Self::new(rng);
        self.best = best;
    }

    pub fn pipes(&self) -> [Pipe; 2] {
        self.pipes
    }

    pub fn bird_rect(&self) -> Rect {
        bird_at(self.bird_y).unwrap_or(Rect {
            x: BIRD_X as u16,
            y: 0,
            w: BIRD_W as u16,
            h: BIRD_H as u16,
        })
    }

    pub fn redraw(&self) -> Redraw {
        if self.scene_dirty {
            Redraw::Full
        } else if self.state == FlapState::Playing {
            Redraw::Live
        } else {
            Redraw::None
        }
    }

    pub fn mark_painted(&mut self) {
        self.prev_bird_y = self.bird_y;
        self.prev_pipes = self.pipes;
        self.prev_score = self.score;
        self.scene_dirty = false;
        self.committed = true;
    }

    pub fn flap(&mut self) {
        match self.state {
            FlapState::Ready => {
                self.state = FlapState::Playing;
                self.bird_v = FLAP_V;
                self.best_at_run_start = self.best;
                self.new_best = false;
                self.scene_dirty = true;
            }
            FlapState::Playing => self.bird_v = FLAP_V,
            FlapState::Dead => self.reset(),
        }
    }

    pub fn tick(&mut self) {
        if self.state != FlapState::Playing {
            return;
        }
        if self.committed {
            self.prev_bird_y = self.bird_y;
            self.prev_pipes = self.pipes;
            self.prev_score = self.score;
            self.committed = false;
        } else {
            self.scene_dirty = true;
        }
        self.bird_v = self.bird_v.saturating_add(GRAVITY);
        if self.bird_v > 8 {
            self.bird_v = 8;
        }
        self.bird_y = self.bird_y.saturating_add(self.bird_v);

        let floor = WORLD_H - GROUND - BIRD_H;
        if self.bird_y < 0 || self.bird_y > floor {
            self.die();
            return;
        }

        for i in 0..2 {
            self.pipes[i].x -= SCROLL;
            if self.pipes[i].x + PIPE_W < 0 {
                let other = self.pipes[1 - i].x;
                self.pipes[i].x = other + PIPE_SPACE;
                self.pipes[i].gap_y = self.next_gap();
                self.scored[i] = false;
                self.scene_dirty = true;
            }
            if !self.scored[i] && self.pipes[i].x + PIPE_W < BIRD_X {
                self.scored[i] = true;
                self.score = self.score.saturating_add(1);
                if self.score > self.best {
                    self.best = self.score;
                }
            }
            if self.hits_pipe(self.pipes[i]) {
                self.die();
                return;
            }
        }
    }

    fn die(&mut self) {
        self.state = FlapState::Dead;
        self.scene_dirty = true;
        self.new_best = self.score > self.best_at_run_start;
        if self.score > self.best {
            self.best = self.score;
        }
        if self.bird_y > WORLD_H - GROUND - BIRD_H {
            self.bird_y = WORLD_H - GROUND - BIRD_H;
        }
        if self.bird_y < 0 {
            self.bird_y = 0;
        }
    }

    fn hits_pipe(&self, p: Pipe) -> bool {
        let bx = BIRD_X;
        let by = self.bird_y;
        if bx + BIRD_W <= p.x || bx >= p.x + PIPE_W {
            return false;
        }
        by < p.gap_y || by + BIRD_H > p.gap_y + GAP
    }

    fn next_gap(&mut self) -> i16 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        let lo = 28i16;
        let hi = WORLD_H - GROUND - GAP - 24;
        lo + (x % (hi - lo) as u32) as i16
    }

    /// Sprite patches for a live tick. Host tests lock the SPI byte count.
    pub fn live_ops(&self) -> Vec<FillOp, 32> {
        let mut ops = Vec::new();
        for i in 0..2 {
            let prev = self.prev_pipes[i];
            let now = self.pipes[i];
            if prev.x == now.x && prev.gap_y == now.gap_y {
                continue;
            }
            let dx = prev.x - now.x;
            if dx > 0 && dx <= PIPE_W && prev.gap_y == now.gap_y {
                // Leftward scroll: only the vacated trailing strip and the
                // new leading strip. A second Live paint with dx==0 used to
                // sky `x+PIPE_W-SCROLL`, punching a 2 px hole in the wall.
                pipe_column(&mut ops, prev.x + PIPE_W - dx, prev.gap_y, dx, COL_SKY);
                pipe_column(&mut ops, now.x, now.gap_y, dx, COL_PIPE);
            } else {
                pipe_column(&mut ops, prev.x, prev.gap_y, PIPE_W, COL_SKY);
                pipe_column(&mut ops, now.x, now.gap_y, PIPE_W, COL_PIPE);
            }
        }
        if self.bird_y != self.prev_bird_y {
            if let Some(old) = bird_at(self.prev_bird_y) {
                push_fill(&mut ops, BIRD_X, self.prev_bird_y, BIRD_W, BIRD_H, COL_SKY);
                for p in self.prev_pipes {
                    restore_pipe(&mut ops, p, old);
                }
                for p in self.pipes {
                    restore_pipe(&mut ops, p, old);
                }
            }
        }
        push_fill(&mut ops, BIRD_X, self.bird_y, BIRD_W, BIRD_H, COL_BIRD);
        push_fill(&mut ops, BIRD_X + 8, self.bird_y + 2, 2, 2, COL_EYE);
        if self.score != self.prev_score {
            if let Some(sr) = clip_i16(SCORE_X, SCORE_Y, SCORE_W, SCORE_H) {
                push_fill(&mut ops, SCORE_X, SCORE_Y, SCORE_W, SCORE_H, COL_SKY);
                for p in self.pipes {
                    restore_pipe(&mut ops, p, sr);
                }
            }
        }
        ops
    }

    pub fn live_spi_bytes(&self) -> u32 {
        let mut px = 0u32;
        for op in self.live_ops() {
            px = px.saturating_add(u32::from(op.rect.w).saturating_mul(u32::from(op.rect.h)));
        }
        px.saturating_mul(2)
    }

    pub fn paint(&self, draw: &mut dyn Draw, vp: Rect, mode: Redraw, pal: Palette) {
        match mode {
            Redraw::None => {}
            Redraw::Full => self.paint_full(draw, vp, pal),
            Redraw::Live => self.paint_live(draw, vp, pal),
        }
    }

    fn paint_full(&self, draw: &mut dyn Draw, vp: Rect, pal: Palette) {
        draw.fill(vp, COL_SKY);
        let gy = vp.y + (WORLD_H - GROUND) as u16;
        draw.fill(
            Rect {
                x: vp.x,
                y: gy,
                w: vp.w,
                h: GROUND as u16,
            },
            COL_GROUND,
        );

        for pipe in self.pipes {
            blit_pipe(draw, vp, pipe);
        }

        blit(draw, vp, self.bird_rect(), COL_BIRD);
        if let Some(eye) = clip_i16(BIRD_X + 8, self.bird_y + 2, 2, 2) {
            blit(draw, vp, eye, COL_EYE);
        }
        self.paint_score(draw, vp, pal);

        match self.state {
            FlapState::Ready => {
                center_text(
                    draw,
                    vp,
                    vp.y + 120,
                    "OK to flap",
                    pal.label,
                    COL_SKY,
                    false,
                );
                if self.best > 0 {
                    let mut best = heapless::String::<16>::new();
                    let _ = write!(best, "best {}", self.best);
                    center_text(
                        draw,
                        vp,
                        vp.y + 140,
                        best.as_str(),
                        pal.secondary,
                        COL_SKY,
                        false,
                    );
                }
            }
            FlapState::Dead => self.paint_dead_card(draw, vp, pal),
            FlapState::Playing => {}
        }
    }

    fn paint_dead_card(&self, draw: &mut dyn Draw, vp: Rect, pal: Palette) {
        let card = Rect {
            x: vp.x.saturating_add(CARD_X),
            y: vp.y.saturating_add(CARD_Y),
            w: CARD_W,
            h: CARD_H,
        };
        draw.fill(card, pal.grouped);
        let title = if self.new_best { "new best" } else { "down" };
        center_text(
            draw,
            vp,
            card.y + 14,
            title,
            if self.new_best { pal.accent } else { pal.label },
            pal.grouped,
            false,
        );
        let mut score = heapless::String::<16>::new();
        let _ = write!(score, "{}", self.score);
        center_text(
            draw,
            vp,
            card.y + 40,
            score.as_str(),
            pal.label,
            pal.grouped,
            true,
        );
        let mut best = heapless::String::<16>::new();
        let _ = write!(best, "best {}", self.best);
        center_text(
            draw,
            vp,
            card.y + 78,
            best.as_str(),
            pal.secondary,
            pal.grouped,
            false,
        );
        center_text(
            draw,
            vp,
            card.y + 100,
            "OK retry",
            pal.label,
            pal.grouped,
            false,
        );
    }

    fn paint_live(&self, draw: &mut dyn Draw, vp: Rect, pal: Palette) {
        for op in self.live_ops() {
            blit(draw, vp, op.rect, op.rgb565);
        }
        if self.score != self.prev_score {
            self.paint_score(draw, vp, pal);
        }
    }

    fn paint_score(&self, draw: &mut dyn Draw, vp: Rect, pal: Palette) {
        let mut score = heapless::String::<16>::new();
        let _ = write!(score, "{}", self.score);
        draw.text(
            vp.x.saturating_add(SCORE_X as u16),
            vp.y.saturating_add(SCORE_Y as u16),
            score.as_str(),
            pal.label,
            COL_SKY,
        );
    }
}

fn bird_at(y: i16) -> Option<Rect> {
    clip_i16(BIRD_X, y, BIRD_W, BIRD_H)
}

fn blit(draw: &mut dyn Draw, vp: Rect, r: Rect, rgb: u16) {
    draw.fill(
        Rect {
            x: vp.x.saturating_add(r.x),
            y: vp.y.saturating_add(r.y),
            w: r.w,
            h: r.h,
        },
        rgb,
    );
}

fn blit_pipe(draw: &mut dyn Draw, vp: Rect, pipe: Pipe) {
    let px = vp.x.saturating_add(pipe.x.max(0) as u16);
    let mut pw = PIPE_W as u16;
    if pipe.x < 0 {
        let cut = (-pipe.x) as u16;
        if cut >= pw {
            return;
        }
        pw -= cut;
    }
    if px >= vp.x + vp.w {
        return;
    }
    if px + pw > vp.x + vp.w {
        pw = vp.x + vp.w - px;
    }
    let gap_top = vp.y + pipe.gap_y.max(0) as u16;
    let top_h = pipe.gap_y.max(0) as u16;
    let gy = vp.y + (WORLD_H - GROUND) as u16;
    if top_h > 0 {
        draw.fill(
            Rect {
                x: px,
                y: vp.y,
                w: pw,
                h: top_h,
            },
            COL_PIPE,
        );
    }
    let bot_y = gap_top.saturating_add(GAP as u16);
    if bot_y < gy {
        draw.fill(
            Rect {
                x: px,
                y: bot_y,
                w: pw,
                h: gy.saturating_sub(bot_y),
            },
            COL_PIPE,
        );
    }
}

fn pipe_column(ops: &mut Vec<FillOp, 32>, x: i16, gap_y: i16, w: i16, rgb: u16) {
    let floor = WORLD_H - GROUND;
    push_fill(ops, x, 0, w, gap_y, rgb);
    let bot = gap_y + GAP;
    push_fill(ops, x, bot, w, floor - bot, rgb);
}

fn restore_pipe(ops: &mut Vec<FillOp, 32>, p: Pipe, hole: Rect) {
    if let Some(top) = clip_i16(p.x, 0, PIPE_W, p.gap_y) {
        if let Some(hit) = top.intersection(hole) {
            let _ = ops.push(FillOp {
                rect: hit,
                rgb565: COL_PIPE,
            });
        }
    }
    let bot_y = p.gap_y + GAP;
    if let Some(bot) = clip_i16(p.x, bot_y, PIPE_W, WORLD_H - GROUND - bot_y) {
        if let Some(hit) = bot.intersection(hole) {
            let _ = ops.push(FillOp {
                rect: hit,
                rgb565: COL_PIPE,
            });
        }
    }
}

fn push_fill(ops: &mut Vec<FillOp, 32>, x: i16, y: i16, w: i16, h: i16, rgb: u16) {
    if let Some(rect) = clip_i16(x, y, w, h) {
        let _ = ops.push(FillOp { rect, rgb565: rgb });
    }
}

fn clip_i16(x: i16, y: i16, w: i16, h: i16) -> Option<Rect> {
    if w <= 0 || h <= 0 {
        return None;
    }
    let x2 = x.saturating_add(w);
    let y2 = y.saturating_add(h);
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = x2.min(WORLD_W);
    let y1 = y2.min(WORLD_H);
    if x1 <= x0 || y1 <= y0 {
        None
    } else {
        Some(Rect {
            x: x0 as u16,
            y: y0 as u16,
            w: (x1 - x0) as u16,
            h: (y1 - y0) as u16,
        })
    }
}

fn center_text(draw: &mut dyn Draw, vp: Rect, y: u16, s: &str, fg: u16, bg: u16, scale_2x: bool) {
    let cw = if scale_2x { FONT_2X } else { FONT_W };
    let w = (s.len() as u16).saturating_mul(cw);
    let x = vp.x.saturating_add(vp.w.saturating_sub(w) / 2);
    if scale_2x {
        draw.text_2x(x, y, s, fg, bg);
    } else {
        draw.text(x, y, s, fg, bg);
    }
}

pub fn read_best(store: &dyn Store) -> u16 {
    let mut buf = [0u8; 2];
    match store.get(BEST_KEY, &mut buf) {
        Some(2) => u16::from_le_bytes(buf),
        _ => 0,
    }
}

pub fn write_best(store: &mut dyn Store, n: u16) {
    let _ = store.put(BEST_KEY, &n.to_le_bytes());
}

/// Immediate flap. Click-on-release is too late: long-OK is home, and a
/// 30 ms SPI fill used to swallow the release sample.
pub fn is_flap_input(ev: ButtonEvent) -> bool {
    matches!(
        ev,
        ButtonEvent::Press(Key::Ok) | ButtonEvent::Press(Key::Up)
    )
}

/// Compile-time check: a live frame must not be a playfield fill.
pub const fn live_budget_holds() -> bool {
    rgb565_bytes(WORLD_W as u16, WORLD_H as u16) > LIVE_SPI_BUDGET
        && crate::compositor::spi_time_us(LIVE_SPI_BUDGET) < 5_000
}
