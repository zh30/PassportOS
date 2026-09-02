//! Non-overlapping tiled surfaces under a fixed status bar. Portrait 240×320.

use heapless::Vec;

use crate::board::{LCD_H, LCD_SPI_HZ, LCD_W};

pub const STATUS_BAR_H: u16 = 22;
pub const WORKSPACE_COUNT: usize = 2;
pub const TILES_PER_WORKSPACE: usize = 2;

/// RGB565 bytes for a rectangle. ST7789 GRAM is the framebuffer; the CPU
/// only ships this many bytes per fill. A 240×298 content wipe is 143 040 B
/// ≈ 29 ms at 40 MHz — longer than the 20 ms input period.
pub const fn rgb565_bytes(w: u16, h: u16) -> u32 {
    (w as u32).saturating_mul(h as u32).saturating_mul(2)
}

/// Ceiling on a live (non-scene) frame. 8 KiB ≈ 1.6 ms at 40 MHz SPI2, so
/// ADC sampling is never skipped for animation. Scene changes may exceed this.
pub const LIVE_SPI_BUDGET: u32 = 8 * 1024;

/// SPI time in microseconds for `bytes` at the panel clock (command overhead
/// not included; fills dominate). Uses u64 so a playfield fill does not wrap.
pub const fn spi_time_us(bytes: u32) -> u32 {
    let ns = (bytes as u64).saturating_mul(8).saturating_mul(1_000_000);
    (ns / LCD_SPI_HZ as u64) as u32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub const fn contains(self, x: u16, y: u16) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    pub fn overlaps(self, other: Rect) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }

    pub const fn bottom(self) -> u16 {
        self.y + self.h
    }

    /// Overlapping region, if any. Used to clip app drawing to a tile.
    pub fn intersection(self, other: Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let x2 = self.x.saturating_add(self.w).min(other.x.saturating_add(other.w));
        let y2 = self.y.saturating_add(self.h).min(other.y.saturating_add(other.h));
        if x2 > x && y2 > y {
            Some(Rect {
                x,
                y,
                w: x2 - x,
                h: y2 - y,
            })
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileLayout {
    pub status: Rect,
    pub tiles: Vec<Rect, TILES_PER_WORKSPACE>,
}

/// Layout `n` non-overlapping tiles in the content area below the status bar.
/// `n == 0` yields an empty content list (status only). `n >= 2` splits top/bottom.
pub fn layout_tiles(n: usize) -> TileLayout {
    layout_tiles_in(n, LCD_W, LCD_H)
}

pub fn layout_tiles_in(n: usize, width: u16, height: u16) -> TileLayout {
    let status = Rect {
        x: 0,
        y: 0,
        w: width,
        h: STATUS_BAR_H.min(height),
    };
    let mut tiles = Vec::new();
    if height <= STATUS_BAR_H {
        return TileLayout { status, tiles };
    }
    let content_y = status.h;
    let content_h = height - status.h;
    match n {
        0 => {}
        1 => {
            let _ = tiles.push(Rect {
                x: 0,
                y: content_y,
                w: width,
                h: content_h,
            });
        }
        _ => {
            let top_h = content_h / 2;
            let bot_h = content_h - top_h;
            let _ = tiles.push(Rect {
                x: 0,
                y: content_y,
                w: width,
                h: top_h,
            });
            let _ = tiles.push(Rect {
                x: 0,
                y: content_y + top_h,
                w: width,
                h: bot_h,
            });
        }
    }
    TileLayout { status, tiles }
}

impl TileLayout {
    pub fn is_non_overlapping(&self) -> bool {
        for (i, a) in self.tiles.iter().enumerate() {
            if a.overlaps(self.status) {
                return false;
            }
            for b in self.tiles.iter().skip(i + 1) {
                if a.overlaps(*b) {
                    return false;
                }
            }
        }
        true
    }
}
