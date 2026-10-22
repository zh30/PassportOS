//! ST7789P3 240×320, SPI mode 0, invert-on, vendor porch/gamma from the official BSP.

use core::sync::atomic::{AtomicUsize, Ordering};

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::SpiBus;

use crate::font::{FONT_H, FONT_W, glyph};

static AUDIO_SERVICE: AtomicUsize = AtomicUsize::new(0);

/// Firmware registers an I2S pump so a long fill can feed TX / drain RX
/// without ending the SPI window. `None` clears it.
pub fn set_audio_service(service: Option<fn()>) {
    AUDIO_SERVICE.store(service.map(|f| f as usize).unwrap_or(0), Ordering::Relaxed);
}

fn service_audio() {
    let ptr = AUDIO_SERVICE.load(Ordering::Relaxed);
    if ptr != 0 {
        let f: fn() = unsafe { core::mem::transmute(ptr) };
        f();
    }
}

/// 512 RGB565 pixels. Static so the 72 KB radio heap stays untouched.
/// The UI task is the only painter.
struct FillBuf(core::cell::UnsafeCell<[u8; 1024]>);
unsafe impl Sync for FillBuf {}
static FILL_BUF: FillBuf = FillBuf(core::cell::UnsafeCell::new([0; 1024]));

/// One text run: up to a full 240 px row at 2× (20 cells × 12 × 16 × 2).
struct SpanBuf(core::cell::UnsafeCell<[u8; 20 * 12 * 16 * 2]>);
unsafe impl Sync for SpanBuf {}
static SPAN_BUF: SpanBuf = SpanBuf(core::cell::UnsafeCell::new([0; 20 * 12 * 16 * 2]));

pub const WIDTH: u16 = 240;
pub const HEIGHT: u16 = 320;

pub struct St7789<SPI, DC, CS> {
    spi: SPI,
    dc: DC,
    cs: CS,
}

impl<SPI, DC, CS, E> St7789<SPI, DC, CS>
where
    SPI: SpiBus<u8, Error = E>,
    DC: OutputPin,
    CS: OutputPin,
{
    pub fn new(spi: SPI, dc: DC, cs: CS) -> Self {
        Self { spi, dc, cs }
    }

    pub fn init(&mut self, delay: &mut impl DelayNs, fill: u16) -> Result<(), E> {
        self.command(0x01, &[])?; // SWRESET (no RST pin)
        delay.delay_ns(150_000_000);
        self.command(0x11, &[])?; // SLPOUT
        delay.delay_ns(120_000_000);
        self.command(0x3A, &[0x55])?; // COLMOD 16-bit
        // Official ST7789P3 vendor sequence (bsp_display.c).
        self.command(0xB2, &[0x05, 0x05, 0x00, 0x33, 0x33])?;
        self.command(0xB7, &[0x35])?;
        self.command(0xBB, &[0x21])?;
        self.command(0xC0, &[0x2C])?;
        self.command(0xC2, &[0x01])?;
        self.command(0xC3, &[0x0B])?;
        self.command(0xC4, &[0x20])?;
        self.command(0xC6, &[0x0F])?;
        self.command(0xD0, &[0xA7, 0xA1])?;
        self.command(0xD0, &[0xA4, 0xA1])?;
        self.command(0xD6, &[0xA1])?;
        self.command(
            0xE0,
            &[
                0xD0, 0x04, 0x08, 0x0A, 0x09, 0x05, 0x2D, 0x43, 0x49, 0x09, 0x16, 0x15, 0x26, 0x2B,
            ],
        )?;
        self.command(
            0xE1,
            &[
                0xD0, 0x03, 0x09, 0x0A, 0x0A, 0x06, 0x2E, 0x44, 0x40, 0x3A, 0x15, 0x15, 0x26, 0x2A,
            ],
        )?;
        delay.delay_ns(10_000_000);
        self.command(0x21, &[])?; // INVON
        self.command(0x36, &[0x00])?; // MADCTL MX=MY=0
        // Fill GRAM *before* DISPON so the first lit frame is not random (boot 花屏).
        self.fill_screen(fill)?;
        self.command(0x29, &[])?; // DISPON
        delay.delay_ns(20_000_000);
        Ok(())
    }

    fn select(&mut self, on: bool) {
        if on {
            let _ = self.cs.set_low();
        } else {
            let _ = self.cs.set_high();
        }
    }

    fn command(&mut self, cmd: u8, data: &[u8]) -> Result<(), E> {
        self.select(true);
        let _ = self.dc.set_low();
        self.spi.write(&[cmd])?;
        if !data.is_empty() {
            let _ = self.dc.set_high();
            self.spi.write(data)?;
        }
        self.spi.flush()?;
        self.select(false);
        Ok(())
    }

    /// CASET/RASET then RAMWR with CS held for the following pixel bytes.
    fn begin_pixels(&mut self, x0: u16, y0: u16, x1: u16, y1: u16) -> Result<(), E> {
        self.command(
            0x2A,
            &[(x0 >> 8) as u8, x0 as u8, (x1 >> 8) as u8, x1 as u8],
        )?;
        self.command(
            0x2B,
            &[(y0 >> 8) as u8, y0 as u8, (y1 >> 8) as u8, y1 as u8],
        )?;
        self.select(true);
        let _ = self.dc.set_low();
        self.spi.write(&[0x2C])?;
        let _ = self.dc.set_high();
        Ok(())
    }

    fn end_pixels(&mut self) -> Result<(), E> {
        self.spi.flush()?;
        self.select(false);
        Ok(())
    }

    pub fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, color: u16) -> Result<(), E> {
        if w == 0 || h == 0 || x >= WIDTH || y >= HEIGHT {
            return Ok(());
        }
        let x1 = x.saturating_add(w - 1).min(WIDTH - 1);
        let y1 = y.saturating_add(h - 1).min(HEIGHT - 1);
        if x1 < x || y1 < y {
            return Ok(());
        }
        self.begin_pixels(x, y, x1, y1)?;
        let hi = (color >> 8) as u8;
        let lo = color as u8;
        let buf = unsafe { &mut *FILL_BUF.0.get() };
        for chunk in buf.chunks_exact_mut(2) {
            chunk[0] = hi;
            chunk[1] = lo;
        }
        let total = (x1 - x + 1) as u32 * (y1 - y + 1) as u32;
        let mut left = total;
        while left > 0 {
            let n = core::cmp::min(left, 512) as usize;
            self.spi.write(&buf[..n * 2])?;
            left -= n as u32;
            // Between chunks, while CS stays in data mode. Keeps I2S clocks
            // alive across a playfield fill.
            service_audio();
        }
        self.end_pixels()
    }

    pub fn fill_screen(&mut self, color: u16) -> Result<(), E> {
        self.fill_rect(0, 0, WIDTH, HEIGHT, color)
    }

    pub fn draw_text(&mut self, x: u16, y: u16, text: &str, fg: u16, bg: u16) -> Result<(), E> {
        self.draw_text_scaled(x, y, text, fg, bg, 1)
    }

    pub fn draw_text_2x(&mut self, x: u16, y: u16, text: &str, fg: u16, bg: u16) -> Result<(), E> {
        self.draw_text_scaled(x, y, text, fg, bg, 2)
    }

    /// One SPI window per run of whole glyphs. A clipped cell is skipped so
    /// RAMWR length always matches the window (a short write 花屏s this panel).
    fn draw_text_scaled(
        &mut self,
        mut x: u16,
        y: u16,
        text: &str,
        fg: u16,
        bg: u16,
        scale: u16,
    ) -> Result<(), E> {
        let cell_w = FONT_W.saturating_mul(scale);
        let cell_h = FONT_H.saturating_mul(scale);
        if cell_w == 0 || cell_h == 0 || y >= HEIGHT {
            return Ok(());
        }
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if x.saturating_add(cell_w) > WIDTH {
                break;
            }
            let room = ((WIDTH - x) / cell_w) as usize;
            let pix = (cell_w as usize)
                .saturating_mul(cell_h as usize)
                .saturating_mul(2);
            let buf_cap = if pix == 0 { 0 } else { (20 * 12 * 16 * 2) / pix };
            let n = room.min(buf_cap).min(bytes.len() - i);
            if n == 0 {
                break;
            }
            self.blit_span(x, y, &bytes[i..i + n], fg, bg, scale)?;
            x = x.saturating_add(cell_w.saturating_mul(n as u16));
            i += n;
        }
        Ok(())
    }

    fn blit_span(
        &mut self,
        x: u16,
        y: u16,
        chars: &[u8],
        fg: u16,
        bg: u16,
        scale: u16,
    ) -> Result<(), E> {
        let cell_w = FONT_W.saturating_mul(scale);
        let cell_h = FONT_H.saturating_mul(scale);
        let w = cell_w.saturating_mul(chars.len() as u16);
        let h = cell_h;
        if w == 0 || h == 0 || y.saturating_add(h) > HEIGHT || x.saturating_add(w) > WIDTH {
            return Ok(());
        }
        let pixels = unsafe { &mut *SPAN_BUF.0.get() };
        let mut i = 0;
        for row in 0..FONT_H {
            for _dup_row in 0..scale {
                for &c in chars {
                    let g = glyph(c);
                    for col in 0..FONT_W {
                        let on = col < 5 && (g[col as usize] & (1 << row)) != 0;
                        let color = if on { fg } else { bg };
                        let hi = (color >> 8) as u8;
                        let lo = color as u8;
                        for _dup_col in 0..scale {
                            pixels[i] = hi;
                            pixels[i + 1] = lo;
                            i += 2;
                        }
                    }
                }
            }
        }
        let x1 = x + w - 1;
        let y1 = y + h - 1;
        self.begin_pixels(x, y, x1, y1)?;
        self.spi.write(&pixels[..i])?;
        self.end_pixels()?;
        service_audio();
        Ok(())
    }
}
