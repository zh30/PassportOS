//! Boo: shout to scare ghosts in a window. Stay quiet for the sleeping king.
//!
//! Device path: [`crate::shell::Shell::tick_mic`] → [`crate::app::AppLifecycle::Mic`].
//! Host game tests may still call [`BooWorld::feed_mic`] with an 8-bit level.

use core::fmt::Write as _;

use heapless::Vec;

use crate::api::{Draw, Store};
use crate::app::AppId;
use crate::board::Key;
use crate::compositor::{LIVE_SPI_BUDGET, Rect, rgb565_bytes};
use crate::flap::{FillOp, Redraw};
use crate::input::ButtonEvent;
use crate::mic::{LOUD_LEVEL, MicEvent, MicGate, ROAR_LEVEL};
use crate::theme::Palette;

pub const BOO_APP_ID: AppId = AppId(7);
pub const BEST_KEY: &[u8] = b"boo";

pub const WORLD_W: i16 = 240;
pub const WORLD_H: i16 = 298;
pub const GHOST_W: i16 = 18;
pub const GHOST_H: i16 = 14;
pub const FACE_W: i16 = 14;
pub const FACE_H: i16 = 16;
pub const FACE_X: i16 = 8;
pub const SCARE_X: i16 = 40;
pub const SCARE_W: i16 = 28;
pub const MUTE_TICKS: u8 = 30;
pub const TWO_LANE_AT: u16 = 4;
pub const MAX_MOBS: usize = 4;

pub const COL_BG: u16 = 0x10A2;
pub const COL_GHOST: u16 = 0xC618;
pub const COL_KING: u16 = 0xFE60;
pub const COL_FACE: u16 = 0x07FD;
pub const COL_SCARE: u16 = 0x39C7;
pub const COL_MUTE: u16 = 0xF800;

const CARD_X: u16 = 16;
const CARD_W: u16 = 208;
const CARD_Y: u16 = 86;
const CARD_H: u16 = 124;
const FONT_W: u16 = 6;
const FONT_2X: u16 = 12;
const SCORE_X: i16 = 8;
const SCORE_Y: i16 = 6;
pub const SCORE_W: i16 = 36;
pub const SCORE_H: i16 = 8;
pub const MUTE_BAR_X: i16 = 52;
pub const MUTE_BAR_Y: i16 = 6;
pub const MUTE_BAR_W: i16 = 40;
pub const MUTE_BAR_H: i16 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BooState {
    Ready,
    Playing,
    Dead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MobKind {
    Ghost,
    King,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mob {
    pub x: i16,
    pub lane: u8,
    pub kind: MobKind,
}

#[derive(Clone, Debug)]
pub struct BooWorld {
    pub state: BooState,
    pub score: u16,
    pub best: u16,
    pub lane: u8,
    roar_at: u8,
    loud_at: u8,
    mute: u8,
    mobs: Vec<Mob, MAX_MOBS>,
    prev_mobs: Vec<Mob, MAX_MOBS>,
    prev_lane: u8,
    prev_score: u16,
    prev_mute: u8,
    spawn_cd: u8,
    rng: u32,
    gate: MicGate,
    noise_acc: u16,
    noise_n: u8,
    scene_dirty: bool,
    committed: bool,
    best_at_run_start: u16,
    new_best: bool,
}

impl Default for BooWorld {
    fn default() -> Self {
        Self::new(0xB00B_0001)
    }
}

impl BooWorld {
    pub const fn new(seed: u32) -> Self {
        Self {
            state: BooState::Ready,
            score: 0,
            best: 0,
            lane: 0,
            roar_at: ROAR_LEVEL,
            loud_at: LOUD_LEVEL,
            mute: 0,
            mobs: Vec::new(),
            prev_mobs: Vec::new(),
            prev_lane: 0,
            prev_score: 0,
            prev_mute: 0,
            spawn_cd: 20,
            rng: seed | 1,
            gate: MicGate::new(),
            noise_acc: 0,
            noise_n: 0,
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

    pub fn is_muted(&self) -> bool {
        self.mute > 0
    }

    pub fn two_lanes(&self) -> bool {
        self.score >= TWO_LANE_AT
    }

    pub fn mobs(&self) -> &[Mob] {
        &self.mobs
    }

    pub fn roar_at(&self) -> u8 {
        self.roar_at
    }

    pub fn reset(&mut self) {
        let best = self.best;
        let rng = self.rng;
        *self = Self::new(rng);
        self.best = best;
    }

    pub fn redraw(&self) -> Redraw {
        if self.scene_dirty {
            Redraw::Full
        } else if self.state == BooState::Playing {
            Redraw::Live
        } else {
            Redraw::None
        }
    }

    pub fn mark_painted(&mut self) {
        self.prev_mobs = self.mobs.clone();
        self.prev_lane = self.lane;
        self.prev_score = self.score;
        self.prev_mute = self.mute;
        self.scene_dirty = false;
        self.committed = true;
    }

    pub fn place(&mut self, x: i16, lane: u8, kind: MobKind) {
        let _ = self.mobs.push(Mob {
            x,
            lane: lane.min(1),
            kind,
        });
    }

    pub fn set_lane(&mut self, lane: u8) {
        self.lane = lane.min(1);
    }

    pub fn start(&mut self) {
        if self.state == BooState::Ready {
            self.state = BooState::Playing;
            self.best_at_run_start = self.best;
            self.new_best = false;
            self.mute = 0;
            self.spawn_cd = 12;
            self.scene_dirty = true;
        }
    }

    pub fn roar(&mut self, too_loud: bool) {
        match self.state {
            BooState::Ready => self.start(),
            BooState::Dead => self.reset(),
            BooState::Playing => self.try_scare(too_loud),
        }
    }

    pub fn nudge_lane(&mut self, ev: ButtonEvent) {
        if self.state != BooState::Playing || !self.two_lanes() {
            return;
        }
        match ev {
            ButtonEvent::Press(Key::Up) | ButtonEvent::Click(Key::Up) => {
                self.lane = 0;
            }
            ButtonEvent::Press(Key::Down) | ButtonEvent::Click(Key::Down) => {
                self.lane = 1;
            }
            _ => {}
        }
    }

    pub fn feed_mic(&mut self, level: u8) {
        if self.state == BooState::Ready {
            self.calibrate(level);
        }
        match self.gate.feed(level, self.roar_at, self.loud_at) {
            None => {}
            Some(MicEvent::Loud) => self.roar(false),
            Some(MicEvent::Peak) => self.roar(true),
        }
    }

    /// Device path: Shell already edge-triggered the shout.
    pub fn on_mic(&mut self, ev: MicEvent) {
        match ev {
            MicEvent::Loud => self.roar(false),
            MicEvent::Peak => self.roar(true),
        }
    }

    fn calibrate(&mut self, level: u8) {
        if self.noise_n == 255 {
            return;
        }
        self.noise_acc = self.noise_acc.saturating_add(u16::from(level));
        self.noise_n = self.noise_n.saturating_add(1);
        if self.noise_n >= 16 {
            let avg = (self.noise_acc / u16::from(self.noise_n)) as u8;
            self.roar_at = avg.saturating_add(24).clamp(32, 100);
            self.loud_at = self.roar_at.saturating_add(72).clamp(160, 250);
        }
    }

    fn try_scare(&mut self, too_loud: bool) {
        if self.mute > 0 {
            return;
        }
        let king_on_screen = self.mobs.iter().any(|m| m.kind == MobKind::King);
        let king_in_zone = self
            .mobs
            .iter()
            .any(|m| m.kind == MobKind::King && in_scare(m.x));
        if king_in_zone || (too_loud && king_on_screen) {
            self.die();
            return;
        }
        if let Some(i) = self
            .mobs
            .iter()
            .position(|m| m.kind == MobKind::Ghost && m.lane == self.lane && in_scare(m.x))
        {
            let _ = self.mobs.swap_remove(i);
            self.score = self.score.saturating_add(1);
            if self.score > self.best {
                self.best = self.score;
            }
        } else {
            self.mute = MUTE_TICKS;
        }
    }

    pub fn tick(&mut self) {
        if self.state != BooState::Playing {
            return;
        }
        if self.committed {
            // Keep last-painted prev_*. Device apply_notes (roar/nudge) runs
            // after mark_painted and before this tick; recopying here would
            // hide scare, score, mute, and lane live diffs.
            self.committed = false;
        } else {
            self.scene_dirty = true;
        }
        if self.mute > 0 {
            self.mute -= 1;
        }
        let speed = self.speed();
        let mut i = 0;
        while i < self.mobs.len() {
            self.mobs[i].x = self.mobs[i].x.saturating_sub(speed);
            // Left of the scare window without a hit: ghost got through.
            if self.mobs[i].x + GHOST_W <= SCARE_X {
                if self.mobs[i].kind == MobKind::Ghost {
                    self.die();
                    return;
                }
                let _ = self.mobs.swap_remove(i);
                continue;
            }
            i += 1;
        }
        self.maybe_spawn();
    }

    fn speed(&self) -> i16 {
        (2 + (self.score / 6) as i16).min(5)
    }

    fn maybe_spawn(&mut self) {
        if self.spawn_cd > 0 {
            self.spawn_cd -= 1;
            return;
        }
        if self.mobs.len() >= MAX_MOBS {
            return;
        }
        if self.mobs.iter().any(|m| m.x > WORLD_W - 56) {
            return;
        }
        let lane = if self.two_lanes() {
            (self.next_rng() % 2) as u8
        } else {
            0
        };
        let king = self.score >= 3 && (self.next_rng() % 5) == 0;
        self.place(
            WORLD_W,
            lane,
            if king { MobKind::King } else { MobKind::Ghost },
        );
        let period = 55u8.saturating_sub(self.score.min(30) as u8).max(22);
        self.spawn_cd = period;
    }

    fn next_rng(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    fn die(&mut self) {
        self.state = BooState::Dead;
        self.scene_dirty = true;
        self.new_best = self.score > self.best_at_run_start;
        if self.score > self.best {
            self.best = self.score;
        }
    }

    pub fn live_ops(&self) -> Vec<FillOp, 32> {
        let mut ops = Vec::new();
        let mut used = [false; MAX_MOBS];
        for prev in self.prev_mobs.iter() {
            let py = lane_y(prev.lane);
            let mut paired: Option<usize> = None;
            for (i, now) in self.mobs.iter().enumerate() {
                if used[i] || now.lane != prev.lane || now.kind != prev.kind {
                    continue;
                }
                let dx = prev.x - now.x;
                if dx > 0 && dx <= GHOST_W {
                    paired = Some(i);
                    break;
                }
            }
            if let Some(i) = paired {
                used[i] = true;
                let now = self.mobs[i];
                let dx = prev.x - now.x;
                let rgb = mob_rgb(now.kind);
                // Leftward scroll: vacated trailing strip + new leading strip.
                push_fill(&mut ops, prev.x + GHOST_W - dx, py, dx, GHOST_H, COL_BG);
                push_fill(&mut ops, now.x, py, dx, GHOST_H, rgb);
                push_fill(&mut ops, prev.x + 4, py + 4, 2, 2, rgb);
                push_fill(&mut ops, prev.x + 10, py + 4, 2, 2, rgb);
                push_fill(&mut ops, now.x + 4, py + 4, 2, 2, COL_BG);
                push_fill(&mut ops, now.x + 10, py + 4, 2, 2, COL_BG);
            } else {
                push_fill(&mut ops, prev.x, py, GHOST_W, GHOST_H, COL_BG);
            }
        }
        for (i, now) in self.mobs.iter().enumerate() {
            if used[i] {
                continue;
            }
            let y = lane_y(now.lane);
            let rgb = mob_rgb(now.kind);
            push_fill(&mut ops, now.x, y, GHOST_W, GHOST_H, rgb);
            push_fill(&mut ops, now.x + 4, y + 4, 2, 2, COL_BG);
            push_fill(&mut ops, now.x + 10, y + 4, 2, 2, COL_BG);
        }
        if self.prev_lane != self.lane {
            push_fill(
                &mut ops,
                FACE_X,
                lane_y(self.prev_lane),
                FACE_W,
                FACE_H,
                COL_BG,
            );
            push_fill(
                &mut ops,
                FACE_X,
                lane_y(self.lane),
                FACE_W,
                FACE_H,
                COL_FACE,
            );
        }
        if self.score != self.prev_score {
            push_fill(&mut ops, SCORE_X, SCORE_Y, SCORE_W, SCORE_H, COL_BG);
        }
        if (self.mute > 0) != (self.prev_mute > 0) {
            push_fill(
                &mut ops,
                MUTE_BAR_X,
                MUTE_BAR_Y,
                MUTE_BAR_W,
                MUTE_BAR_H,
                if self.mute > 0 { COL_MUTE } else { COL_BG },
            );
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
        draw.fill(vp, COL_BG);
        let sx = vp.x.saturating_add(SCARE_X as u16);
        draw.fill(
            Rect {
                x: sx,
                y: vp.y,
                w: 2,
                h: vp.h,
            },
            COL_SCARE,
        );
        blit(
            draw,
            vp,
            FACE_X,
            lane_y(self.lane),
            FACE_W,
            FACE_H,
            COL_FACE,
        );
        for m in self.mobs.iter() {
            let rgb = if m.kind == MobKind::King {
                COL_KING
            } else {
                COL_GHOST
            };
            blit(draw, vp, m.x, lane_y(m.lane), GHOST_W, GHOST_H, rgb);
            blit(draw, vp, m.x + 4, lane_y(m.lane) + 4, 2, 2, COL_BG);
            blit(draw, vp, m.x + 10, lane_y(m.lane) + 4, 2, 2, COL_BG);
        }
        self.paint_score(draw, vp, pal);
        if self.mute > 0 {
            blit(
                draw, vp, MUTE_BAR_X, MUTE_BAR_Y, MUTE_BAR_W, MUTE_BAR_H, COL_MUTE,
            );
        }
        match self.state {
            BooState::Ready => {
                center_text(
                    draw,
                    vp,
                    vp.y + 120,
                    "SHOUT to boo",
                    pal.label,
                    COL_BG,
                    false,
                );
                center_text(
                    draw,
                    vp,
                    vp.y + 138,
                    "OK start",
                    pal.secondary,
                    COL_BG,
                    false,
                );
                if self.best > 0 {
                    let mut best = heapless::String::<16>::new();
                    let _ = write!(best, "best {}", self.best);
                    center_text(
                        draw,
                        vp,
                        vp.y + 156,
                        best.as_str(),
                        pal.secondary,
                        COL_BG,
                        false,
                    );
                }
            }
            BooState::Dead => self.paint_dead_card(draw, vp, pal),
            BooState::Playing => {}
        }
    }

    fn paint_live(&self, draw: &mut dyn Draw, vp: Rect, pal: Palette) {
        for op in self.live_ops() {
            blit_rect(draw, vp, op.rect, op.rgb565);
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
            COL_BG,
        );
    }

    fn paint_dead_card(&self, draw: &mut dyn Draw, vp: Rect, pal: Palette) {
        let card = Rect {
            x: vp.x.saturating_add(CARD_X),
            y: vp.y.saturating_add(CARD_Y),
            w: CARD_W,
            h: CARD_H,
        };
        draw.fill(card, pal.grouped);
        center_text(draw, vp, card.y + 16, "boo", pal.accent, pal.grouped, true);
        let mut score = heapless::String::<16>::new();
        let _ = write!(score, "{}", self.score);
        center_text(
            draw,
            vp,
            card.y + 48,
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
            card.y + 80,
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
}

pub fn lane_y(lane: u8) -> i16 {
    if lane == 0 { 72 } else { 176 }
}

pub const fn in_scare(x: i16) -> bool {
    x + GHOST_W > SCARE_X && x < SCARE_X + SCARE_W
}

fn mob_rgb(kind: MobKind) -> u16 {
    if kind == MobKind::King {
        COL_KING
    } else {
        COL_GHOST
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
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = x.saturating_add(w).min(WORLD_W);
    let y1 = y.saturating_add(h).min(WORLD_H);
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

fn blit(draw: &mut dyn Draw, vp: Rect, x: i16, y: i16, w: i16, h: i16, rgb: u16) {
    if let Some(r) = clip_i16(x, y, w, h) {
        blit_rect(draw, vp, r, rgb);
    }
}

fn blit_rect(draw: &mut dyn Draw, vp: Rect, r: Rect, rgb: u16) {
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

pub fn is_boo_roar_key(ev: ButtonEvent) -> bool {
    matches!(ev, ButtonEvent::Press(Key::Ok))
}

pub fn is_boo_lane_key(ev: ButtonEvent) -> bool {
    matches!(
        ev,
        ButtonEvent::Press(Key::Up)
            | ButtonEvent::Press(Key::Down)
            | ButtonEvent::Click(Key::Up)
            | ButtonEvent::Click(Key::Down)
    )
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
    rgb565_bytes(WORLD_W as u16, WORLD_H as u16) > LIVE_SPI_BUDGET
        && crate::compositor::spi_time_us(LIVE_SPI_BUDGET) < 5_000
}
