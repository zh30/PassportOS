//! Boot mark: a little card drops in, then the system name types on.
//!
//! Dirty-rect only — never a 240×80 (or 240×320) RAMWR. Those stalls are
//! what made the mark hitch on this ST7789.

use crate::api::Draw;
use crate::board::{OS_NAME, LCD_W};
use crate::compositor::Rect;
use crate::theme::Palette;

/// Matches [`crate::board::FRAME_TICK_MS`]. Remainder-sleep in firmware keeps
/// this period including SPI, not SPI-then-wait.
pub const BOOT_TICK_MS: u32 = 20;

const DROP_TICKS: u8 = 14;
const TYPE_START: u8 = 16;
const TYPE_EVERY: u8 = 2;
const UNDERLINE_START: u8 = 36;
const UNDERLINE_TICKS: u8 = 12;
const HOLD_TICKS: u8 = 56;

const STAMP_START_Y: u16 = 52;
const STAMP_LAND_Y: u16 = 92;
/// Was 18×14 — a speck on 240×320. Logo sits above the typed name.
pub const STAMP_W: u16 = 48;
pub const STAMP_H: u16 = 40;
const STAMP_SQUASH_H: u16 = 32;
const P_W: u16 = 20;
const P_H: u16 = 24;
const TITLE_Y: u16 = 148;
const FONT_2X_W: u16 = 12;
const FONT_2X_H: u16 = 16;

pub fn title() -> &'static str {
    OS_NAME
}

pub const fn title_width() -> u16 {
    (OS_NAME.len() as u16).saturating_mul(FONT_2X_W)
}

pub const fn title_x() -> u16 {
    LCD_W.saturating_sub(title_width()) / 2
}

fn stamp_x() -> u16 {
    title_x() + title_width() / 2 - STAMP_W / 2
}

fn stamp_rect(y: u16, h: u16) -> Rect {
    Rect {
        x: stamp_x(),
        y,
        w: STAMP_W,
        h,
    }
}

fn p_origin(stamp_y: u16, stamp_h: u16) -> (u16, u16) {
    (
        stamp_x() + (STAMP_W.saturating_sub(P_W)) / 2,
        stamp_y + stamp_h.saturating_sub(P_H) / 2,
    )
}

/// Chunky P, ~3× the 6×8 glyph. Fill-rects only — no 3× font RAMWR.
fn paint_p(draw: &mut dyn Draw, ox: u16, oy: u16, rgb: u16) {
    draw.fill(
        Rect {
            x: ox,
            y: oy,
            w: 6,
            h: P_H,
        },
        rgb,
    );
    draw.fill(
        Rect {
            x: ox,
            y: oy,
            w: P_W,
            h: 6,
        },
        rgb,
    );
    draw.fill(
        Rect {
            x: ox + 14,
            y: oy,
            w: 6,
            h: 14,
        },
        rgb,
    );
    draw.fill(
        Rect {
            x: ox,
            y: oy + 10,
            w: P_W,
            h: 6,
        },
        rgb,
    );
}

fn paint_stamp(draw: &mut dyn Draw, y: u16, h: u16, pal: Palette) {
    draw.fill(stamp_rect(y, h), pal.grouped);
    if h >= P_H {
        let (ox, oy) = p_origin(y, h);
        paint_p(draw, ox, oy, pal.accent);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootAnim {
    tick: u8,
    done: bool,
    painted: bool,
    prev_stamp_y: u16,
    prev_stamp_h: u16,
    prev_letters: u8,
    prev_uw: u16,
}

impl Default for BootAnim {
    fn default() -> Self {
        Self::new()
    }
}

impl BootAnim {
    pub const fn new() -> Self {
        Self {
            tick: 0,
            done: false,
            painted: false,
            prev_stamp_y: STAMP_START_Y,
            prev_stamp_h: STAMP_H,
            prev_letters: 0,
            prev_uw: 0,
        }
    }

    pub fn skip(&mut self) {
        self.done = true;
    }

    pub fn is_done(self) -> bool {
        self.done
    }

    pub fn tick(&mut self) {
        if self.done {
            return;
        }
        self.tick = self.tick.saturating_add(1);
        if self.tick >= HOLD_TICKS {
            self.done = true;
        }
    }

    /// Commit the last paint so the next frame is dirty-rect, not a band wipe.
    pub fn mark_painted(&mut self) {
        self.prev_stamp_y = self.stamp_y();
        self.prev_stamp_h = self.stamp_h();
        self.prev_letters = self.letters();
        self.prev_uw = self.underline_w();
        self.painted = true;
    }

    /// How many ASCII letters of [`title`] are visible this frame.
    pub fn letters(self) -> u8 {
        if self.tick < TYPE_START {
            0
        } else {
            let n = (self.tick - TYPE_START) / TYPE_EVERY + 1;
            n.min(OS_NAME.len() as u8)
        }
    }

    /// Public so host tests can lock the ease (more than four jumpy steps).
    pub fn stamp_y(self) -> u16 {
        if self.tick >= DROP_TICKS {
            if self.tick == DROP_TICKS {
                STAMP_LAND_Y + 2
            } else {
                STAMP_LAND_Y
            }
        } else {
            let n = u32::from(self.tick);
            let d = u32::from(DROP_TICKS);
            let dist = u32::from(STAMP_LAND_Y.saturating_sub(STAMP_START_Y));
            let eased = dist.saturating_mul(2 * d * n - n * n) / (d * d);
            STAMP_START_Y + eased as u16
        }
    }

    fn stamp_h(self) -> u16 {
        if self.tick == DROP_TICKS {
            STAMP_SQUASH_H
        } else {
            STAMP_H
        }
    }

    fn underline_w(self) -> u16 {
        if self.tick < UNDERLINE_START {
            0
        } else {
            let t = u16::from(self.tick.saturating_sub(UNDERLINE_START));
            let tw = title_width();
            t.saturating_mul(tw)
                .checked_div(u16::from(UNDERLINE_TICKS))
                .unwrap_or(tw)
                .min(tw)
        }
    }

    pub fn paint(self, draw: &mut dyn Draw, pal: Palette) {
        if self.done {
            return;
        }

        let sy = self.stamp_y();
        let sh = self.stamp_h();
        if !self.painted {
            paint_stamp(draw, sy, sh, pal);
        } else if self.prev_stamp_y != sy || self.prev_stamp_h != sh {
            if self.prev_stamp_h == sh && sy > self.prev_stamp_y {
                // Drop: only the vacated top and new bottom, then slide the P.
                let dy = sy - self.prev_stamp_y;
                draw.fill(
                    Rect {
                        x: stamp_x(),
                        y: self.prev_stamp_y,
                        w: STAMP_W,
                        h: dy,
                    },
                    pal.bg,
                );
                draw.fill(
                    Rect {
                        x: stamp_x(),
                        y: self.prev_stamp_y + self.prev_stamp_h,
                        w: STAMP_W,
                        h: dy,
                    },
                    pal.grouped,
                );
                let (ox, old_oy) = p_origin(self.prev_stamp_y, self.prev_stamp_h);
                paint_p(draw, ox, old_oy, pal.grouped);
                let (_, oy) = p_origin(sy, sh);
                paint_p(draw, ox, oy, pal.accent);
            } else {
                draw.fill(
                    stamp_rect(self.prev_stamp_y, self.prev_stamp_h),
                    pal.bg,
                );
                paint_stamp(draw, sy, sh, pal);
            }
        }

        let n = self.letters();
        let prev_n = if self.painted { self.prev_letters } else { 0 };
        if n > prev_n {
            let start = prev_n as usize;
            let x = title_x() + u16::from(prev_n).saturating_mul(FONT_2X_W);
            draw.text_2x(x, TITLE_Y, &OS_NAME[start..n as usize], pal.label, pal.bg);
        }

        let uw = self.underline_w();
        let prev_uw = if self.painted { self.prev_uw } else { 0 };
        if uw > prev_uw {
            draw.fill(
                Rect {
                    x: title_x() + prev_uw,
                    y: TITLE_Y + FONT_2X_H + 4,
                    w: uw - prev_uw,
                    h: 2,
                },
                pal.accent,
            );
        }
    }
}
