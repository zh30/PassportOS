//! ST7789 adapter for the unified [`passport_core::api::Draw`] trait.

use passport_core::api::Draw;
use passport_core::compositor::Rect;
use passport_core::STATUS_BAR_H;

use crate::st7789::{St7789, HEIGHT, WIDTH};

pub struct LcdDraw<'a, SPI, DC, CS> {
    lcd: &'a mut St7789<SPI, DC, CS>,
    clip: Rect,
}

impl<'a, SPI, DC, CS> LcdDraw<'a, SPI, DC, CS> {
    pub fn new(lcd: &'a mut St7789<SPI, DC, CS>, clip: Rect) -> Self {
        Self { lcd, clip }
    }
}

pub fn content_rect() -> Rect {
    Rect {
        x: 0,
        y: STATUS_BAR_H,
        w: WIDTH,
        h: HEIGHT.saturating_sub(STATUS_BAR_H),
    }
}

impl<SPI, DC, CS, E> Draw for LcdDraw<'_, SPI, DC, CS>
where
    SPI: embedded_hal::spi::SpiBus<u8, Error = E>,
    DC: embedded_hal::digital::OutputPin,
    CS: embedded_hal::digital::OutputPin,
{
    fn fill(&mut self, r: Rect, rgb565: u16) {
        if r.w == 0 || r.h == 0 {
            return;
        }
        let _ = self.lcd.fill_rect(r.x, r.y, r.w, r.h, rgb565);
    }

    fn text(&mut self, x: u16, y: u16, s: &str, fg: u16, bg: u16) {
        let _ = self.lcd.draw_text(x, y, s, fg, bg);
    }

    fn text_2x(&mut self, x: u16, y: u16, s: &str, fg: u16, bg: u16) {
        let _ = self.lcd.draw_text_2x(x, y, s, fg, bg);
    }

    fn clip(&self) -> Rect {
        self.clip
    }
}
