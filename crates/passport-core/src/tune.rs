//! Instrument tuner: pick a string, play it, watch cents.
//!
//! Device path: I2S PCM → [`crate::pitch::PitchBuf`] → [`TuneWorld::listen`].

use core::fmt::Write as _;

use heapless::Vec;

use crate::app::AppId;
use crate::board::Key;
use crate::compositor::{LIVE_SPI_BUDGET, Rect, rgb565_bytes};
use crate::flap::{FillOp, Redraw};
use crate::input::ButtonEvent;
use crate::pitch::{PITCH_RATE, amdf_hz, cents, fold_octave};
use crate::theme::Palette;

pub const TUNE_APP_ID: AppId = AppId(8);

pub const WORLD_W: i16 = 240;
pub const WORLD_H: i16 = 298;
pub const COL_BG: u16 = 0x10A2;
pub const COL_OK: u16 = 0x07E0;
pub const COL_SHARP: u16 = 0xF800;
pub const COL_FLAT: u16 = 0x07FF;
pub const COL_NEEDLE: u16 = 0xFFE0;
pub const COL_TRACK: u16 = 0x39C7;
pub const IN_TUNE_CENTS: i16 = 8;
const BAR_X: i16 = 20;
const BAR_Y: i16 = 168;
const BAR_W: i16 = 200;
const BAR_H: i16 = 8;
const NEEDLE_W: i16 = 4;
const NEEDLE_H: i16 = 22;
const FONT_W: u16 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instrument {
    Guitar,
    Ukulele,
    Violin,
}

impl Instrument {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Guitar => "guitar",
            Self::Ukulele => "ukulele",
            Self::Violin => "violin",
        }
    }

    pub const fn strings(self) -> &'static [TuneString] {
        match self {
            Self::Guitar => &GUITAR,
            Self::Ukulele => &UKULELE,
            Self::Violin => &VIOLIN,
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::Guitar => Self::Ukulele,
            Self::Ukulele => Self::Violin,
            Self::Violin => Self::Guitar,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TuneString {
    pub name: &'static str,
    pub hz: u16,
}

const GUITAR: [TuneString; 6] = [
    TuneString { name: "E2", hz: 82 },
    TuneString {
        name: "A2",
        hz: 110,
    },
    TuneString {
        name: "D3",
        hz: 147,
    },
    TuneString {
        name: "G3",
        hz: 196,
    },
    TuneString {
        name: "B3",
        hz: 247,
    },
    TuneString {
        name: "E4",
        hz: 330,
    },
];
const UKULELE: [TuneString; 4] = [
    TuneString {
        name: "G4",
        hz: 392,
    },
    TuneString {
        name: "C4",
        hz: 262,
    },
    TuneString {
        name: "E4",
        hz: 330,
    },
    TuneString {
        name: "A4",
        hz: 440,
    },
];
const VIOLIN: [TuneString; 4] = [
    TuneString {
        name: "G3",
        hz: 196,
    },
    TuneString {
        name: "D4",
        hz: 294,
    },
    TuneString {
        name: "A4",
        hz: 440,
    },
    TuneString {
        name: "E5",
        hz: 659,
    },
];

#[derive(Clone, Debug)]
pub struct TuneWorld {
    pub instrument: Instrument,
    pub string: u8,
    hz: u16,
    prev_hz: u16,
    prev_cents: i16,
    prev_string: u8,
    scene_dirty: bool,
}

impl Default for TuneWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl TuneWorld {
    pub const fn new() -> Self {
        Self {
            instrument: Instrument::Guitar,
            string: 0,
            hz: 0,
            prev_hz: 0,
            prev_cents: 0,
            prev_string: 0,
            scene_dirty: true,
        }
    }

    pub fn target(&self) -> TuneString {
        let s = self.instrument.strings();
        s[self.string as usize % s.len()]
    }

    pub fn target_hz(&self) -> u16 {
        self.target().hz
    }

    pub fn hz(&self) -> u16 {
        self.hz
    }

    pub fn cents(&self) -> i16 {
        if self.hz == 0 {
            0
        } else {
            cents(self.hz, self.target_hz())
        }
    }

    pub fn in_tune(&self) -> bool {
        self.hz > 0 && self.cents().unsigned_abs() <= IN_TUNE_CENTS as u16
    }

    pub fn cycle_instrument(&mut self) {
        self.instrument = self.instrument.next();
        self.string = 0;
        self.hz = 0;
        self.scene_dirty = true;
    }

    pub fn nudge_string(&mut self, ev: ButtonEvent) {
        let n = self.instrument.strings().len() as u8;
        match ev {
            ButtonEvent::Press(Key::Up) | ButtonEvent::Click(Key::Up) => {
                self.string = if self.string == 0 {
                    n - 1
                } else {
                    self.string - 1
                };
                self.hz = 0;
                self.scene_dirty = true;
            }
            ButtonEvent::Press(Key::Down) | ButtonEvent::Click(Key::Down) => {
                self.string = (self.string + 1) % n;
                self.hz = 0;
                self.scene_dirty = true;
            }
            _ => {}
        }
    }

    /// Search near the selected string so AMDF does not grab an octave.
    pub fn listen(&mut self, samples: &[i16]) {
        let t = self.target_hz();
        let lo = t.saturating_mul(85) / 100;
        let hi = t.saturating_mul(115) / 100;
        self.hz = amdf_hz(samples, PITCH_RATE, lo.max(70), hi.max(lo + 1)).unwrap_or(0);
        if self.hz == 0 {
            if let Some(wide) = amdf_hz(samples, PITCH_RATE, 70, 900) {
                let folded = fold_octave(wide, t);
                let c = cents(folded, t).unsigned_abs();
                if c <= 50 {
                    self.hz = folded;
                }
            }
        }
    }

    pub fn feed_hz(&mut self, hz: u16) {
        self.hz = if hz == 0 {
            0
        } else {
            fold_octave(hz, self.target_hz())
        };
    }

    pub fn redraw(&self) -> Redraw {
        if self.scene_dirty {
            Redraw::Full
        } else {
            Redraw::Live
        }
    }

    pub fn mark_painted(&mut self) {
        self.prev_hz = self.hz;
        self.prev_cents = self.cents();
        self.prev_string = self.string;
        self.scene_dirty = false;
    }

    pub fn live_ops(&self) -> Vec<FillOp, 16> {
        let mut ops = Vec::new();
        if self.prev_hz == self.hz && (self.hz > 0) == (self.prev_hz > 0) {
            return ops;
        }
        if self.prev_hz > 0 {
            let ox = needle_x(self.prev_cents);
            push_fill(&mut ops, ox, BAR_Y - 8, NEEDLE_W, NEEDLE_H, COL_BG);
            push_fill(&mut ops, ox, BAR_Y, NEEDLE_W, BAR_H, COL_TRACK);
        }
        if self.hz > 0 {
            let nx = needle_x(self.cents());
            let rgb = if self.in_tune() {
                COL_OK
            } else if self.cents() > 0 {
                COL_SHARP
            } else {
                COL_FLAT
            };
            push_fill(&mut ops, nx, BAR_Y - 8, NEEDLE_W, NEEDLE_H, rgb);
        }
        // Readout plate (not the whole playfield). Text is drawn on top.
        push_fill(&mut ops, 40, 208, 160, 12, COL_BG);
        ops
    }

    pub fn live_spi_bytes(&self) -> u32 {
        let mut px = 0u32;
        for op in self.live_ops() {
            px = px.saturating_add(u32::from(op.rect.w).saturating_mul(u32::from(op.rect.h)));
        }
        px.saturating_mul(2)
    }

    pub fn paint(&self, draw: &mut dyn crate::api::Draw, vp: Rect, mode: Redraw, pal: Palette) {
        match mode {
            Redraw::None => {}
            Redraw::Full => self.paint_full(draw, vp, pal),
            Redraw::Live => self.paint_live(draw, vp, pal),
        }
    }

    fn paint_full(&self, draw: &mut dyn crate::api::Draw, vp: Rect, pal: Palette) {
        draw.fill(vp, COL_BG);
        center(draw, vp, vp.y + 16, "TUNE", pal.accent, COL_BG, true);
        center(
            draw,
            vp,
            vp.y + 48,
            self.instrument.name(),
            pal.label,
            COL_BG,
            false,
        );
        self.paint_strings(draw, vp, pal);
        draw.fill(
            Rect {
                x: vp.x.saturating_add(BAR_X as u16),
                y: vp.y.saturating_add(BAR_Y as u16),
                w: BAR_W as u16,
                h: BAR_H as u16,
            },
            COL_TRACK,
        );
        self.paint_needle(draw, vp);
        self.paint_readout(draw, vp, pal);
        center(
            draw,
            vp,
            vp.y + 248,
            "OK instrument",
            pal.secondary,
            COL_BG,
            false,
        );
        center(
            draw,
            vp,
            vp.y + 266,
            "UP/DOWN string",
            pal.secondary,
            COL_BG,
            false,
        );
    }

    fn paint_live(&self, draw: &mut dyn crate::api::Draw, vp: Rect, pal: Palette) {
        for op in self.live_ops() {
            blit_rect(draw, vp, op.rect, op.rgb565);
        }
        if self.hz != self.prev_hz {
            self.paint_readout(draw, vp, pal);
        }
    }

    fn paint_strings(&self, draw: &mut dyn crate::api::Draw, vp: Rect, pal: Palette) {
        let s = self.instrument.strings();
        let mut x = vp.x.saturating_add(16);
        let y = vp.y.saturating_add(80);
        for (i, st) in s.iter().enumerate() {
            let sel = i == self.string as usize;
            let fg = if sel { pal.accent } else { pal.secondary };
            draw.text(x, y, st.name, fg, COL_BG);
            x = x.saturating_add((st.name.len() as u16).saturating_mul(FONT_W) + 10);
        }
    }

    fn paint_needle(&self, draw: &mut dyn crate::api::Draw, vp: Rect) {
        if self.hz == 0 {
            return;
        }
        let nx = needle_x(self.cents());
        let rgb = if self.in_tune() {
            COL_OK
        } else if self.cents() > 0 {
            COL_SHARP
        } else {
            COL_FLAT
        };
        blit_rect(
            draw,
            vp,
            Rect {
                x: nx.max(0) as u16,
                y: (BAR_Y - 8).max(0) as u16,
                w: NEEDLE_W as u16,
                h: NEEDLE_H as u16,
            },
            rgb,
        );
    }

    fn paint_readout(&self, draw: &mut dyn crate::api::Draw, vp: Rect, pal: Palette) {
        let y = vp.y.saturating_add(208);
        if self.hz == 0 {
            center(draw, vp, y, "play a string", pal.secondary, COL_BG, false);
            return;
        }
        let mut line = heapless::String::<24>::new();
        let c = self.cents();
        if self.in_tune() {
            let _ = write!(line, "{} Hz  in tune", self.hz);
        } else if c > 0 {
            let _ = write!(line, "{} Hz  {}c sharp", self.hz, c);
        } else {
            let _ = write!(line, "{} Hz  {}c flat", self.hz, c.unsigned_abs());
        }
        let fg = if self.in_tune() { COL_OK } else { pal.label };
        center(draw, vp, y, line.as_str(), fg, COL_BG, false);
    }
}

fn needle_x(cents: i16) -> i16 {
    let dx = cents.clamp(-50, 50) as i16 * 2;
    BAR_X + BAR_W / 2 + dx - NEEDLE_W / 2
}

fn push_fill(ops: &mut Vec<FillOp, 16>, x: i16, y: i16, w: i16, h: i16, rgb: u16) {
    if w <= 0 || h <= 0 {
        return;
    }
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = x.saturating_add(w).min(WORLD_W);
    let y1 = y.saturating_add(h).min(WORLD_H);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let _ = ops.push(FillOp {
        rect: Rect {
            x: x0 as u16,
            y: y0 as u16,
            w: (x1 - x0) as u16,
            h: (y1 - y0) as u16,
        },
        rgb565: rgb,
    });
}

fn blit_rect(draw: &mut dyn crate::api::Draw, vp: Rect, r: Rect, rgb: u16) {
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

fn center(
    draw: &mut dyn crate::api::Draw,
    vp: Rect,
    y: u16,
    s: &str,
    fg: u16,
    bg: u16,
    scale_2x: bool,
) {
    let cw = if scale_2x { 12 } else { FONT_W };
    let w = (s.len() as u16).saturating_mul(cw);
    let x = vp.x.saturating_add(vp.w.saturating_sub(w) / 2);
    if scale_2x {
        draw.text_2x(x, y, s, fg, bg);
    } else {
        draw.text(x, y, s, fg, bg);
    }
}

pub fn is_tune_string_key(ev: ButtonEvent) -> bool {
    matches!(
        ev,
        ButtonEvent::Click(Key::Up) | ButtonEvent::Click(Key::Down)
    )
}

pub fn is_tune_instrument_key(ev: ButtonEvent) -> bool {
    matches!(ev, ButtonEvent::Click(Key::Ok))
}

pub const fn live_budget_holds() -> bool {
    rgb565_bytes(WORLD_W as u16, WORLD_H as u16) > LIVE_SPI_BUDGET
        && crate::compositor::spi_time_us(LIVE_SPI_BUDGET) < 5_000
}
