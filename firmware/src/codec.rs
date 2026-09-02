//! ES8311 (0x18) and CW2017 (0x63) on the shared I2C0 bus. Never create a second bus.

use embedded_hal::i2c::I2c;
use passport_core::board::{I2C_CW2017_ADDR, I2C_ES8311_ADDR};

const ES8311_INIT: &[(u8, u8)] = &[
    (0x00, 0x1F),
    (0x00, 0x00),
    (0x01, 0x30),
    (0x02, 0x00),
    (0x03, 0x10),
    (0x16, 0x24),
    (0x04, 0x10),
    (0x05, 0x00),
    (0x06, 0x03),
    (0x07, 0x01),
    (0x08, 0xFF),
    (0x09, 0x0C),
    (0x0A, 0x0C),
    (0x0D, 0x01),
    (0x0E, 0x02),
    (0x12, 0x00),
    (0x13, 0x10),
    (0x14, 0x1A),
    (0x17, 0xBF),
    (0x32, 0xBF),
    (0x37, 0x08),
    (0x44, 0x08), // no DAC ref — mic path would otherwise read silence
];

pub fn probe<I: I2c>(i2c: &mut I, addr: u8) -> bool {
    let mut b = [0u8];
    i2c.write_read(addr, &[0x00], &mut b).is_ok()
}

pub fn es8311_init<I: I2c>(i2c: &mut I) -> Result<(), I::Error> {
    for &(reg, val) in ES8311_INIT {
        i2c.write(I2C_ES8311_ADDR, &[reg, val])?;
    }
    Ok(())
}

pub fn cw2017_wake<I: I2c>(i2c: &mut I) -> Result<(), I::Error> {
    i2c.write(I2C_CW2017_ADDR, &[0x08, 0x00]) // CONFIG = normal
}

pub fn cw2017_soc_mv<I: I2c>(i2c: &mut I) -> Option<(u8, u16)> {
    let mut soc = [0u8; 2];
    let mut vcell = [0u8; 2];
    i2c.write_read(I2C_CW2017_ADDR, &[0x04], &mut soc).ok()?;
    i2c.write_read(I2C_CW2017_ADDR, &[0x02], &mut vcell).ok()?;
    let pct = soc[0];
    if pct > 100 {
        return None;
    }
    let raw = (u16::from(vcell[0]) << 8 | u16::from(vcell[1])) & 0x3FFF;
    let mv = (u32::from(raw) * 3125 / 10_000) as u16;
    Some((pct, mv))
}

pub fn i2c_scan_line<I: I2c>(i2c: &mut I) -> heapless::String<96> {
    let mut s = heapless::String::new();
    let es = probe(i2c, I2C_ES8311_ADDR);
    let cw = probe(i2c, I2C_CW2017_ADDR);
    let _ = core::fmt::Write::write_fmt(
        &mut s,
        format_args!(
            "[probe] i2c es8311@0x18={} cw2017@0x63={}",
            if es { "ok" } else { "miss" },
            if cw { "ok" } else { "miss" }
        ),
    );
    s
}
