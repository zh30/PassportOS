//! Host tests for the boot mark. No HAL.

use passport_core::api::{Draw, MeteredDraw, NullDraw};
use passport_core::board::{OS_NAME, LCD_W};
use passport_core::boot::{title, BootAnim, BOOT_TICK_MS, STAMP_H, STAMP_W};
use passport_core::compositor::{Rect, LIVE_SPI_BUDGET};
use passport_core::theme::Palette;

fn vp() -> Rect {
    Rect {
        x: 0,
        y: 0,
        w: LCD_W,
        h: 320,
    }
}

struct TextLog {
    clip: Rect,
    chunks: heapless::Vec<heapless::String<16>, 12>,
}

impl Draw for TextLog {
    fn fill(&mut self, _r: Rect, _c: u16) {}
    fn text(&mut self, _x: u16, _y: u16, s: &str, _fg: u16, _bg: u16) {
        let mut line = heapless::String::new();
        let _ = line.push_str(s);
        let _ = self.chunks.push(line);
    }
    fn text_2x(&mut self, x: u16, y: u16, s: &str, fg: u16, bg: u16) {
        self.text(x, y, s, fg, bg);
    }
    fn clip(&self) -> Rect {
        self.clip
    }
}

struct FillLog {
    clip: Rect,
    max_wh: u32,
}

impl Draw for FillLog {
    fn fill(&mut self, r: Rect, _c: u16) {
        let area = u32::from(r.w).saturating_mul(u32::from(r.h));
        if area > self.max_wh {
            self.max_wh = area;
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
fn title_is_the_os_name() {
    assert_eq!(title(), OS_NAME);
    assert_eq!(OS_NAME, "PassportOS");
    assert_eq!(BOOT_TICK_MS, 20);
}

#[test]
fn logo_is_large_enough_to_read() {
    // Previous stamp was 18×14. The dropping mark is the logo; it has to
    // read as a badge above "PassportOS", not a 6×8 speck.
    assert!(STAMP_W >= 40, "logo width {STAMP_W}");
    assert!(STAMP_H >= 32, "logo height {STAMP_H}");
    assert!(STAMP_W > STAMP_H / 2);
    let a = BootAnim::new();
    let mut fills = FillLog {
        clip: vp(),
        max_wh: 0,
    };
    a.paint(&mut fills, Palette::DARK);
    assert!(
        fills.max_wh >= u32::from(STAMP_W) * u32::from(STAMP_H),
        "first frame must paint the full logo, biggest rect {}",
        fills.max_wh
    );
}

#[test]
fn letters_type_on_then_hold_then_done() {
    let mut a = BootAnim::new();
    assert_eq!(a.letters(), 0);
    assert!(!a.is_done());
    let mut saw = 0u8;
    let mut frames = 0u16;
    while !a.is_done() {
        a.tick();
        frames = frames.saturating_add(1);
        let n = a.letters();
        assert!(n >= saw, "letters must not go backwards");
        if n > saw {
            assert_eq!(n, saw + 1, "typewriter steps one glyph");
            saw = n;
        }
        assert!(frames < 200, "boot mark must finish");
    }
    assert_eq!(saw, OS_NAME.len() as u8);
    assert!(a.is_done());
}

#[test]
fn skip_finishes_immediately() {
    let mut a = BootAnim::new();
    a.skip();
    assert!(a.is_done());
    let mut n = NullDraw::new(vp());
    let mut m = MeteredDraw::new(&mut n);
    a.paint(&mut m, Palette::DARK);
    assert_eq!(m.spi_bytes(), 0, "done mark must not paint");
}

#[test]
fn paint_types_the_name_and_stays_in_a_band() {
    let mut a = BootAnim::new();
    while a.letters() < OS_NAME.len() as u8 {
        a.tick();
        assert!(!a.is_done(), "name must finish typing before hold ends");
    }
    let mut log = TextLog {
        clip: vp(),
        chunks: heapless::Vec::new(),
    };
    a.paint(&mut log, Palette::DARK);
    let typed = log
        .chunks
        .iter()
        .find(|s| s.as_str() != "P")
        .map(|s| s.as_str())
        .unwrap_or("");
    assert_eq!(typed, OS_NAME, "typed {typed:?} chunks={:?}", log.chunks);

    let mut n = NullDraw::new(vp());
    let mut m = MeteredDraw::new(&mut n);
    a.paint(&mut m, Palette::DARK);
    let band = passport_core::rgb565_bytes(LCD_W, 80);
    assert!(
        m.spi_bytes() < band,
        "boot mark must not wipe a 240×80 band, got {}",
        m.spi_bytes()
    );
}

#[test]
fn drop_eases_across_many_steps() {
    let mut a = BootAnim::new();
    let mut ys: heapless::Vec<u16, 24> = heapless::Vec::new();
    let y0 = a.stamp_y();
    for _ in 0..20 {
        let y = a.stamp_y();
        if ys.last().copied() != Some(y) {
            let _ = ys.push(y);
        }
        a.tick();
    }
    assert!(
        ys.len() >= 8,
        "drop must ease, not four jumpy frames: {ys:?}"
    );
    assert!(y0 < *ys.last().unwrap());
}

#[test]
fn live_ticks_stay_in_spi_budget() {
    let mut a = BootAnim::new();
    let mut n = NullDraw::new(vp());
    let mut m = MeteredDraw::new(&mut n);
    a.paint(&mut m, Palette::DARK);
    a.mark_painted();
    assert!(m.spi_bytes() < LIVE_SPI_BUDGET);

    let mut max = 0u32;
    let mut fills = FillLog {
        clip: vp(),
        max_wh: 0,
    };
    while !a.is_done() {
        a.tick();
        let mut n = NullDraw::new(vp());
        let mut m = MeteredDraw::new(&mut n);
        a.paint(&mut m, Palette::DARK);
        a.paint(&mut fills, Palette::DARK);
        a.mark_painted();
        max = max.max(m.spi_bytes());
    }
    assert!(
        max <= LIVE_SPI_BUDGET,
        "live boot frame {max} over {LIVE_SPI_BUDGET}"
    );
    let band = u32::from(LCD_W) * 80;
    assert!(
        fills.max_wh < band,
        "live boot must not fill a 240×80 band, biggest rect {}",
        fills.max_wh
    );
}
