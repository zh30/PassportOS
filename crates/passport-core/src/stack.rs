//! Stack: a slab slides; OK drops it. Overhang is cut. Miss and you are down.
//!
//! Live ticks only move the sliding slab — well inside [`LIVE_SPI_BUDGET`].

use core::fmt::Write as _;

use heapless::Vec;

use crate::api::{Draw, Store};
use crate::app::AppId;
use crate::board::Key;
use crate::compositor::{rgb565_bytes, Rect, LIVE_SPI_BUDGET};
use crate::flap::Redraw;
use crate::input::ButtonEvent;
use crate::theme::Palette;

pub const STACK_APP_ID: AppId = AppId(5);
pub const BEST_KEY: &[u8] = b"stk";

pub const WORLD_W: i16 = 240;
pub const WORLD_H: i16 = 298;
pub const SLAB_H: i16 = 12;
pub const GROUND: i16 = 10;
pub const BASE_W: i16 = 112;
pub const BASE_X: i16 = (WORLD_W - BASE_W) / 2;
pub const MAX_SLABS: usize = 14;
pub const START_SPEED: i16 = 2;
pub const MAX_SPEED: i16 = 6;
pub const MIN_W: i16 = 6;

pub const COL_BG: u16 = 0x10A2;
pub const COL_A: u16 = 0x2945;
pub const COL_B: u16 = 0x07FD;
pub const COL_MOVE: u16 = 0xFE60;
pub const COL_GROUND: u16 = 0x2945;

const CARD_X: u16 = 16;
const CARD_W: u16 = 208;
const CARD_Y: u16 = 86;
const CARD_H: u16 = 124;
const FONT_W: u16 = 6;
const FONT_2X: u16 = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackState {
    Ready,
    Playing,
    Dead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slab {
    pub x: i16,
    pub w: i16,
}

#[derive(Clone, Debug)]
pub struct StackWorld {
    pub state: StackState,
    pub score: u16,
    pub best: u16,
    pub mover: Slab,
    slabs: Vec<Slab, MAX_SLABS>,
    dir: i16,
    speed: i16,
    from_left: bool,
    prev_mover: Slab,
    scene_dirty: bool,
    committed: bool,
    best_at_run_start: u16,
    new_best: bool,
}

impl Default for StackWorld {
    fn default() -> Self {
        Self::new(0x51AC_0001)
    }
}

impl StackWorld {
    pub fn new(seed: u32) -> Self {
        let base = Slab {
            x: BASE_X,
            w: BASE_W,
        };
        let mut this = Self {
            state: StackState::Ready,
            score: 0,
            best: 0,
            mover: base,
            slabs: Vec::new(),
            dir: 1,
            speed: START_SPEED,
            from_left: seed & 1 == 0,
            prev_mover: base,
            scene_dirty: true,
            committed: false,
            best_at_run_start: 0,
            new_best: false,
        };
        this.ensure_base();
        this
    }

    pub fn reset(&mut self) {
        let best = self.best;
        *self = Self::new(0x51AC_0001);
        self.best = best;
        self.ensure_base();
    }

    fn ensure_base(&mut self) {
        if self.slabs.is_empty() {
            let _ = self.slabs.push(Slab {
                x: BASE_X,
                w: BASE_W,
            });
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

    pub fn slabs(&self) -> &[Slab] {
        self.slabs.as_slice()
    }

    pub fn redraw(&self) -> Redraw {
        if self.scene_dirty {
            Redraw::Full
        } else if self.state == StackState::Playing {
            Redraw::Live
        } else {
            Redraw::None
        }
    }

    pub fn mark_painted(&mut self) {
        self.prev_mover = self.mover;
        self.scene_dirty = false;
        self.committed = true;
    }

    pub fn drop(&mut self) {
        self.ensure_base();
        match self.state {
            StackState::Ready => {
                self.state = StackState::Playing;
                self.best_at_run_start = self.best;
                self.new_best = false;
                self.spawn_mover();
                self.scene_dirty = true;
            }
            StackState::Playing => self.place(),
            StackState::Dead => self.reset(),
        }
    }

    pub fn tick(&mut self) {
        if self.state != StackState::Playing {
            return;
        }
        if self.committed {
            self.prev_mover = self.mover;
            self.committed = false;
        } else {
            self.scene_dirty = true;
        }
        self.mover.x = self.mover.x.saturating_add(self.dir.saturating_mul(self.speed));
        if self.mover.x <= 0 {
            self.mover.x = 0;
            self.dir = 1;
        }
        let right = WORLD_W - self.mover.w;
        if self.mover.x >= right {
            self.mover.x = right.max(0);
            self.dir = -1;
        }
    }

    fn place(&mut self) {
        let Some(top) = self.slabs.last().copied() else {
            self.die();
            return;
        };
        let Some(hit) = overlap(self.mover, top) else {
            self.die();
            return;
        };
        if hit.w < MIN_W {
            self.die();
            return;
        }
        if self.slabs.len() == MAX_SLABS {
            let _ = self.slabs.remove(0);
        }
        let _ = self.slabs.push(hit);
        self.score = self.score.saturating_add(1);
        if self.score > self.best {
            self.best = self.score;
        }
        if self.score % 4 == 0 && self.speed < MAX_SPEED {
            self.speed = self.speed.saturating_add(1);
        }
        self.spawn_mover();
        self.scene_dirty = true;
    }

    fn spawn_mover(&mut self) {
        let w = self.slabs.last().map(|s| s.w).unwrap_or(BASE_W);
        self.from_left = !self.from_left;
        if self.from_left {
            self.mover = Slab { x: 0, w };
            self.dir = 1;
        } else {
            self.mover = Slab {
                x: (WORLD_W - w).max(0),
                w,
            };
            self.dir = -1;
        }
        self.prev_mover = self.mover;
    }

    fn die(&mut self) {
        self.state = StackState::Dead;
        self.scene_dirty = true;
        self.new_best = self.score > self.best_at_run_start;
        if self.score > self.best {
            self.best = self.score;
        }
    }

    fn slab_y(index_from_bottom: usize) -> i16 {
        WORLD_H - GROUND - SLAB_H * (index_from_bottom as i16 + 1)
    }

    pub fn mover_y(&self) -> i16 {
        Self::slab_y(self.slabs.len())
    }

    pub fn live_ops(&self) -> Vec<(Rect, u16), 4> {
        let mut ops = Vec::new();
        let y = self.mover_y();
        let prev = self.prev_mover;
        let now = self.mover;
        if prev == now {
            return ops;
        }
        // Same-size slide: only the vacated trailing strip and the new
        // leading strip. Filling the whole slab with COL_BG then COL_MOVE
        // makes the overlap blink on this ST7789.
        if prev.w == now.w && prev.w > 0 {
            if now.x > prev.x {
                let dx = now.x - prev.x;
                if let Some(r) = slab_rect(Slab { x: prev.x, w: dx }, y) {
                    let _ = ops.push((r, COL_BG));
                }
                if let Some(r) = slab_rect(
                    Slab {
                        x: prev.x + prev.w,
                        w: dx,
                    },
                    y,
                ) {
                    let _ = ops.push((r, COL_MOVE));
                }
                return ops;
            }
            if now.x < prev.x {
                let dx = prev.x - now.x;
                if let Some(r) = slab_rect(
                    Slab {
                        x: now.x + now.w,
                        w: dx,
                    },
                    y,
                ) {
                    let _ = ops.push((r, COL_BG));
                }
                if let Some(r) = slab_rect(Slab { x: now.x, w: dx }, y) {
                    let _ = ops.push((r, COL_MOVE));
                }
                return ops;
            }
        }
        if let Some(r) = slab_rect(prev, y) {
            let _ = ops.push((r, COL_BG));
        }
        if let Some(r) = slab_rect(now, y) {
            let _ = ops.push((r, COL_MOVE));
        }
        ops
    }

    pub fn live_spi_bytes(&self) -> u32 {
        let mut px = 0u32;
        for (r, _) in self.live_ops() {
            px = px.saturating_add(u32::from(r.w).saturating_mul(u32::from(r.h)));
        }
        px.saturating_mul(2)
    }

    pub fn paint(&self, draw: &mut dyn Draw, vp: Rect, mode: Redraw, pal: Palette) {
        match mode {
            Redraw::None => {}
            Redraw::Full => self.paint_full(draw, vp, pal),
            Redraw::Live => self.paint_live(draw, vp),
        }
    }

    fn paint_full(&self, draw: &mut dyn Draw, vp: Rect, pal: Palette) {
        draw.fill(vp, COL_BG);
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
        for (i, slab) in self.slabs.iter().enumerate() {
            if let Some(r) = slab_rect(*slab, Self::slab_y(i)) {
                blit(draw, vp, r, if i % 2 == 0 { COL_A } else { COL_B });
            }
        }
        if self.state == StackState::Playing {
            if let Some(r) = slab_rect(self.mover, self.mover_y()) {
                blit(draw, vp, r, COL_MOVE);
            }
        }
        self.paint_score(draw, vp, pal);
        match self.state {
            StackState::Ready => {
                center_text(draw, vp, vp.y + 48, "OK to drop", pal.label, COL_BG, false);
                if self.best > 0 {
                    let mut best = heapless::String::<16>::new();
                    let _ = write!(best, "best {}", self.best);
                    center_text(
                        draw,
                        vp,
                        vp.y + 66,
                        best.as_str(),
                        pal.secondary,
                        COL_BG,
                        false,
                    );
                }
            }
            StackState::Dead => self.paint_dead_card(draw, vp, pal),
            StackState::Playing => {}
        }
    }

    fn paint_live(&self, draw: &mut dyn Draw, vp: Rect) {
        for (r, c) in self.live_ops() {
            blit(draw, vp, r, c);
        }
    }

    fn paint_score(&self, draw: &mut dyn Draw, vp: Rect, pal: Palette) {
        let mut score = heapless::String::<16>::new();
        let _ = write!(score, "{}", self.score);
        draw.text(vp.x + 8, vp.y + 6, score.as_str(), pal.label, COL_BG);
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
        center_text(draw, vp, card.y + 40, score.as_str(), pal.label, pal.grouped, true);
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
        center_text(draw, vp, card.y + 100, "OK retry", pal.label, pal.grouped, false);
    }
}

fn overlap(a: Slab, b: Slab) -> Option<Slab> {
    let x = a.x.max(b.x);
    let x2 = (a.x + a.w).min(b.x + b.w);
    let w = x2 - x;
    if w <= 0 {
        None
    } else {
        Some(Slab { x, w })
    }
}

fn slab_rect(s: Slab, y: i16) -> Option<Rect> {
    if s.w <= 0 {
        return None;
    }
    let x = s.x.max(0) as u16;
    let y = y.max(0) as u16;
    let mut w = s.w as u16;
    if s.x < 0 {
        w = w.saturating_sub((-s.x) as u16);
    }
    if x >= WORLD_W as u16 || y >= WORLD_H as u16 || w == 0 {
        return None;
    }
    let w = w.min((WORLD_W as u16).saturating_sub(x));
    let h = (SLAB_H as u16).min((WORLD_H as u16).saturating_sub(y));
    Some(Rect { x, y, w, h })
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

fn center_text(
    draw: &mut dyn Draw,
    vp: Rect,
    y: u16,
    s: &str,
    fg: u16,
    bg: u16,
    scale_2x: bool,
) {
    let cw = if scale_2x { FONT_2X } else { FONT_W };
    let w = (s.len() as u16).saturating_mul(cw);
    let x = vp.x.saturating_add(vp.w.saturating_sub(w) / 2);
    if scale_2x {
        draw.text_2x(x, y, s, fg, bg);
    } else {
        draw.text(x, y, s, fg, bg);
    }
}

pub fn is_stack_input(ev: ButtonEvent) -> bool {
    matches!(ev, ButtonEvent::Press(Key::Ok))
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

pub const fn live_budget_holds() -> bool {
    rgb565_bytes(BASE_W as u16, SLAB_H as u16).saturating_mul(2) <= LIVE_SPI_BUDGET
}
