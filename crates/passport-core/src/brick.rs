//! Breakout, portrait: paddle on the right, UP/DOWN move it, OK serves.
//!
//! Live ticks erase/draw the ball, paddle, and at most one brick.

use core::fmt::Write as _;

use heapless::Vec;

use crate::api::{Draw, Store};
use crate::app::AppId;
use crate::board::Key;
use crate::compositor::{rgb565_bytes, Rect, LIVE_SPI_BUDGET};
use crate::flap::Redraw;
use crate::input::ButtonEvent;
use crate::theme::Palette;

pub const BRICK_APP_ID: AppId = AppId(6);
pub const BEST_KEY: &[u8] = b"brk";

pub const WORLD_W: i16 = 240;
pub const WORLD_H: i16 = 298;
pub const BALL: i16 = 6;
pub const PAD_W: i16 = 8;
pub const PAD_H: i16 = 40;
pub const PAD_X: i16 = WORLD_W - PAD_W - 4;
pub const PAD_SPEED: i16 = 5;
pub const START_SPEED: i16 = 2;
pub const MAX_SPEED: i16 = 4;

pub const COLS: usize = 4;
pub const ROWS: usize = 6;
pub const BRICK_N: usize = COLS * ROWS;
pub const BRICK_W: u16 = 40;
pub const BRICK_H: u16 = 12;
pub const GAP: u16 = 4;
pub const GRID_X: u16 = 8;
pub const GRID_Y: u16 = 36;

pub const COL_BG: u16 = 0x10A2;
pub const COL_PAD: u16 = 0xEF7D;
pub const COL_BALL: u16 = 0xFE60;

const CARD_X: u16 = 16;
const CARD_W: u16 = 208;
const CARD_Y: u16 = 86;
const CARD_H: u16 = 124;
const FONT_W: u16 = 6;
const FONT_2X: u16 = 12;

const ALL_BRICKS: u32 = (1 << BRICK_N) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrickState {
    Ready,
    Playing,
    Dead,
}

#[derive(Clone, Debug)]
pub struct BrickWorld {
    pub state: BrickState,
    pub score: u16,
    pub best: u16,
    pub ball_x: i16,
    pub ball_y: i16,
    pub pad_y: i16,
    pub vx: i16,
    pub vy: i16,
    alive: u32,
    speed: i16,
    held: Option<Key>,
    prev_ball_x: i16,
    prev_ball_y: i16,
    prev_pad_y: i16,
    cleared: Option<Rect>,
    scene_dirty: bool,
    committed: bool,
    /// Refill already sits on a painted playfield; a 240×298 RAMWR flashes 黑屏.
    skip_wipe: bool,
    best_at_run_start: u16,
    new_best: bool,
}

impl Default for BrickWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl BrickWorld {
    pub const fn new() -> Self {
        let pad_y = (WORLD_H - PAD_H) / 2;
        let ball_x = PAD_X - BALL - 1;
        let ball_y = pad_y + (PAD_H - BALL) / 2;
        Self {
            state: BrickState::Ready,
            score: 0,
            best: 0,
            ball_x,
            ball_y,
            pad_y,
            vx: 0,
            vy: 0,
            alive: ALL_BRICKS,
            speed: START_SPEED,
            held: None,
            prev_ball_x: ball_x,
            prev_ball_y: ball_y,
            prev_pad_y: pad_y,
            cleared: None,
            scene_dirty: true,
            committed: false,
            skip_wipe: false,
            best_at_run_start: 0,
            new_best: false,
        }
    }

    pub fn reset(&mut self) {
        let best = self.best;
        *self = Self::new();
        self.best = best;
    }

    pub fn seed_best(&mut self, n: u16) {
        if n > self.best {
            self.best = n;
        }
    }

    pub fn is_new_best(&self) -> bool {
        self.new_best
    }

    pub fn bricks_left(&self) -> u32 {
        self.alive.count_ones()
    }

    pub fn redraw(&self) -> Redraw {
        if self.scene_dirty {
            Redraw::Full
        } else if self.state == BrickState::Playing || self.held.is_some() {
            Redraw::Live
        } else {
            Redraw::None
        }
    }

    pub fn mark_painted(&mut self) {
        self.prev_ball_x = self.ball_x;
        self.prev_ball_y = self.ball_y;
        self.prev_pad_y = self.pad_y;
        self.cleared = None;
        self.scene_dirty = false;
        self.committed = true;
        self.skip_wipe = false;
    }

    pub fn input(&mut self, ev: ButtonEvent) {
        match ev {
            ButtonEvent::Press(Key::Ok) => self.serve(),
            ButtonEvent::Press(Key::Up) => self.held = Some(Key::Up),
            ButtonEvent::Press(Key::Down) => self.held = Some(Key::Down),
            ButtonEvent::Release(k) if self.held == Some(k) => self.held = None,
            _ => {}
        }
    }

    fn serve(&mut self) {
        match self.state {
            BrickState::Ready => {
                self.state = BrickState::Playing;
                self.best_at_run_start = self.best;
                self.new_best = false;
                self.vx = -self.speed;
                self.vy = if self.pad_y + PAD_H / 2 < WORLD_H / 2 {
                    -self.speed
                } else {
                    self.speed
                };
                self.scene_dirty = true;
            }
            BrickState::Dead => self.reset(),
            BrickState::Playing => {}
        }
    }

    pub fn tick(&mut self) {
        self.cleared = None;
        if self.committed {
            self.prev_ball_x = self.ball_x;
            self.prev_ball_y = self.ball_y;
            self.prev_pad_y = self.pad_y;
            self.committed = false;
        } else if self.state == BrickState::Playing {
            self.scene_dirty = true;
        }
        self.nudge_pad();
        if self.state != BrickState::Playing {
            self.stick_ball();
            return;
        }
        self.ball_x = self.ball_x.saturating_add(self.vx);
        self.ball_y = self.ball_y.saturating_add(self.vy);
        if self.ball_y < 0 {
            self.ball_y = 0;
            self.vy = self.speed;
        }
        if self.ball_y + BALL > WORLD_H {
            self.ball_y = WORLD_H - BALL;
            self.vy = -self.speed;
        }
        if self.ball_x < 0 {
            self.ball_x = 0;
            self.vx = self.speed;
        }
        if self.vx > 0 && self.hits_paddle() {
            self.bounce_paddle();
        } else if self.ball_x + BALL >= WORLD_W {
            self.die();
            return;
        }
        self.hit_bricks();
        if self.alive == 0 {
            self.refill();
        }
    }

    fn nudge_pad(&mut self) {
        let dy = match self.held {
            Some(Key::Up) => -PAD_SPEED,
            Some(Key::Down) => PAD_SPEED,
            _ => return,
        };
        self.pad_y = (self.pad_y + dy).clamp(0, WORLD_H - PAD_H);
        if self.state != BrickState::Playing {
            self.stick_ball();
        }
    }

    fn stick_ball(&mut self) {
        self.ball_x = PAD_X - BALL - 1;
        self.ball_y = self.pad_y + (PAD_H - BALL) / 2;
        self.vx = 0;
        self.vy = 0;
    }

    fn hits_paddle(&self) -> bool {
        aabb(
            self.ball_x,
            self.ball_y,
            BALL,
            BALL,
            PAD_X,
            self.pad_y,
            PAD_W,
            PAD_H,
        )
    }

    fn bounce_paddle(&mut self) {
        self.ball_x = PAD_X - BALL - 1;
        self.vx = -self.speed;
        let mid = self.pad_y + PAD_H / 2;
        let bmid = self.ball_y + BALL / 2;
        self.vy = if bmid < mid - 10 {
            -self.speed
        } else if bmid < mid {
            -1
        } else if bmid < mid + 10 {
            1
        } else {
            self.speed
        };
        if self.vy == 0 {
            self.vy = 1;
        }
    }

    fn hit_bricks(&mut self) {
        for i in 0..BRICK_N {
            let bit = 1u32 << i;
            if self.alive & bit == 0 {
                continue;
            }
            let Some(br) = brick_world(i) else {
                continue;
            };
            if !aabb(
                self.ball_x,
                self.ball_y,
                BALL,
                BALL,
                br.x as i16,
                br.y as i16,
                br.w as i16,
                br.h as i16,
            ) {
                continue;
            }
            self.alive &= !bit;
            self.score = self.score.saturating_add(1);
            if self.score > self.best {
                self.best = self.score;
            }
            self.cleared = Some(br);
            let ox = overlap_len(self.ball_x, BALL, br.x as i16, br.w as i16);
            let oy = overlap_len(self.ball_y, BALL, br.y as i16, br.h as i16);
            if oy <= ox {
                self.vy = -self.vy;
            } else {
                self.vx = -self.vx;
            }
            if self.vx == 0 {
                self.vx = -self.speed;
            }
            if self.vy == 0 {
                self.vy = self.speed;
            }
            return;
        }
    }

    fn refill(&mut self) {
        self.alive = ALL_BRICKS;
        if self.speed < MAX_SPEED {
            self.speed = self.speed.saturating_add(1);
        }
        self.scene_dirty = true;
        self.skip_wipe = true;
    }

    fn die(&mut self) {
        self.state = BrickState::Dead;
        self.scene_dirty = true;
        self.held = None;
        self.new_best = self.score > self.best_at_run_start;
        if self.score > self.best {
            self.best = self.score;
        }
    }

    pub fn live_ops(&self) -> Vec<(Rect, u16), 16> {
        let mut ops = Vec::new();
        let old_ball = ball_rect(self.prev_ball_x, self.prev_ball_y);
        let old_pad = pad_rect(self.prev_pad_y);
        if let Some(r) = old_ball {
            let _ = ops.push((r, COL_BG));
        }
        if let Some(r) = old_pad {
            let _ = ops.push((r, COL_BG));
        }
        if let Some(r) = self.cleared {
            let _ = ops.push((r, COL_BG));
        }
        // Same as Flap restoring pipes: the ball trail is COL_BG and would
        // punch holes in bricks that are still alive.
        if let Some(r) = old_ball {
            self.restore_bricks(&mut ops, r);
        }
        if let Some(r) = old_pad {
            self.restore_bricks(&mut ops, r);
        }
        if let Some(r) = pad_rect(self.pad_y) {
            let _ = ops.push((r, COL_PAD));
        }
        if let Some(r) = ball_rect(self.ball_x, self.ball_y) {
            let _ = ops.push((r, COL_BALL));
        }
        ops
    }

    fn restore_bricks(&self, ops: &mut Vec<(Rect, u16), 16>, hole: Rect) {
        for i in 0..BRICK_N {
            if self.alive & (1 << i) == 0 {
                continue;
            }
            let Some(br) = brick_world(i) else {
                continue;
            };
            if let Some(hit) = br.intersection(hole) {
                let _ = ops.push((hit, brick_color(i)));
            }
        }
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
        if !self.skip_wipe {
            draw.fill(vp, COL_BG);
        }
        for i in 0..BRICK_N {
            if self.alive & (1 << i) == 0 {
                continue;
            }
            if let Some(r) = brick_world(i) {
                blit(draw, vp, r, brick_color(i));
            }
        }
        if let Some(r) = pad_rect(self.pad_y) {
            blit(draw, vp, r, COL_PAD);
        }
        if let Some(r) = ball_rect(self.ball_x, self.ball_y) {
            blit(draw, vp, r, COL_BALL);
        }
        self.paint_score(draw, vp, pal);
        match self.state {
            BrickState::Ready => {
                center_text(draw, vp, vp.y + 200, "OK serve", pal.label, COL_BG, false);
                center_text(draw, vp, vp.y + 216, "UP/DN pad", pal.secondary, COL_BG, false);
                if self.best > 0 {
                    let mut best = heapless::String::<16>::new();
                    let _ = write!(best, "best {}", self.best);
                    center_text(
                        draw,
                        vp,
                        vp.y + 232,
                        best.as_str(),
                        pal.secondary,
                        COL_BG,
                        false,
                    );
                }
            }
            BrickState::Dead => self.paint_dead_card(draw, vp, pal),
            BrickState::Playing => {}
        }
    }

    fn paint_live(&self, draw: &mut dyn Draw, vp: Rect) {
        for (r, c) in self.live_ops() {
            blit(draw, vp, r, c);
        }
        self.paint_score(draw, vp, Palette::DARK);
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

fn brick_world(i: usize) -> Option<Rect> {
    if i >= BRICK_N {
        return None;
    }
    let c = (i % COLS) as u16;
    let r = (i / COLS) as u16;
    Some(Rect {
        x: GRID_X + c * (BRICK_W + GAP),
        y: GRID_Y + r * (BRICK_H + GAP),
        w: BRICK_W,
        h: BRICK_H,
    })
}

fn brick_color(i: usize) -> u16 {
    match i / COLS {
        0 => 0x07FD,
        1 => 0x07E0,
        2 => 0xFE60,
        3 => 0xF800,
        4 => 0x07FD,
        _ => 0x2945,
    }
}

fn ball_rect(x: i16, y: i16) -> Option<Rect> {
    world_rect(x, y, BALL, BALL)
}

fn pad_rect(y: i16) -> Option<Rect> {
    world_rect(PAD_X, y, PAD_W, PAD_H)
}

fn world_rect(x: i16, y: i16, w: i16, h: i16) -> Option<Rect> {
    if w <= 0 || h <= 0 {
        return None;
    }
    let x0 = x.max(0) as u16;
    let y0 = y.max(0) as u16;
    if x0 >= WORLD_W as u16 || y0 >= WORLD_H as u16 {
        return None;
    }
    let w = (w as u16).min((WORLD_W as u16).saturating_sub(x0));
    let h = (h as u16).min((WORLD_H as u16).saturating_sub(y0));
    Some(Rect {
        x: x0,
        y: y0,
        w,
        h,
    })
}

fn aabb(x: i16, y: i16, w: i16, h: i16, x2: i16, y2: i16, w2: i16, h2: i16) -> bool {
    x < x2 + w2 && x2 < x + w && y < y2 + h2 && y2 < y + h
}

fn overlap_len(a: i16, aw: i16, b: i16, bw: i16) -> i16 {
    (a + aw).min(b + bw) - a.max(b)
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
    rgb565_bytes(PAD_W as u16, PAD_H as u16)
        .saturating_mul(2)
        .saturating_add(rgb565_bytes(BALL as u16, BALL as u16).saturating_mul(2))
        .saturating_add(rgb565_bytes(BRICK_W, BRICK_H))
        // Ball 6×6 can nick at most four still-alive bricks; restore those patches.
        .saturating_add(rgb565_bytes(BALL as u16, BALL as u16).saturating_mul(4))
        <= LIVE_SPI_BUDGET
}
