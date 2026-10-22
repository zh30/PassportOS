//! ES8311 (0x18) and CW2017 (0x63) on the shared I2C0 bus. Never create a second bus.

use embedded_hal::i2c::I2c;
use passport_core::ES8311_INIT;
use passport_core::board::{I2C_CW2017_ADDR, I2C_ES8311_ADDR};
use passport_core::es8311;

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

/// ADC clocks + analog mic after I2S TX is providing MCLK/BCLK.
pub fn es8311_start<I: I2c>(i2c: &mut I) -> Result<(), I::Error> {
    for &(reg, val) in es8311::START {
        i2c.write(I2C_ES8311_ADDR, &[reg, val])?;
    }
    Ok(())
}

pub fn es8311_read<I: I2c>(i2c: &mut I, reg: u8) -> Option<u8> {
    let mut b = [0u8];
    i2c.write_read(I2C_ES8311_ADDR, &[reg], &mut b)
        .ok()
        .map(|_| b[0])
}

pub fn es8311_write<I: I2c>(i2c: &mut I, reg: u8, val: u8) -> Result<(), I::Error> {
    i2c.write(I2C_ES8311_ADDR, &[reg, val])
}

/// Speaker level: system volume 0..=100 + mute → DAC register 0x32.
pub fn es8311_set_volume<I: I2c>(i2c: &mut I, vol: u8, muted: bool) -> Result<(), I::Error> {
    es8311_write(
        i2c,
        es8311::REG_DAC_VOLUME,
        es8311::dac_volume(vol, muted),
    )
}

/// Leave sleep, then clear the restart bit so the gauge reloads its profile.
/// Datasheet: write `0x30`, then `0x00`. A short spin sits between the two writes.
pub fn cw2017_wake<I: I2c>(i2c: &mut I) -> Result<(), I::Error> {
    i2c.write(I2C_CW2017_ADDR, &[0x08, 0x30])?;
    for _ in 0..160_000 {
        core::hint::spin_loop();
    }
    i2c.write(I2C_CW2017_ADDR, &[0x08, 0x00])
}

fn cw_reg<I: I2c>(i2c: &mut I, reg: u8) -> Option<u8> {
    let mut b = [0u8; 1];
    i2c.write_read(I2C_CW2017_ADDR, &[reg], &mut b).ok()?;
    Some(b[0])
}

/// Why a poll did not yield a percentage. Logged once, not every tick.
pub enum Cw2017Fail {
    /// Register read NACKed.
    Io,
    /// Gauge answered, but SOC is not a percentage yet (often `0xFF`).
    Soc { soc: u8, frac: u8, vcell: u16, mode: u8 },
}

pub fn cw2017_soc_mv<I: I2c>(i2c: &mut I) -> Result<(u8, u16), Cw2017Fail> {
    let soc = cw_reg(i2c, 0x04).ok_or(Cw2017Fail::Io)?;
    let frac = cw_reg(i2c, 0x05).ok_or(Cw2017Fail::Io)?;
    let vh = cw_reg(i2c, 0x02).ok_or(Cw2017Fail::Io)?;
    let vl = cw_reg(i2c, 0x03).ok_or(Cw2017Fail::Io)?;
    let mode = cw_reg(i2c, 0x08).unwrap_or(0);
    let raw = (u16::from(vh) << 8 | u16::from(vl)) & 0x3FFF;
    let mv = (u32::from(raw) * 3125 / 10_000) as u16;
    // 0 mV is the gauge mid-restart, not an empty cell. SOC above 100
    // (often 0xFE) means the profile has not produced a percentage yet.
    if soc > 100 || mv < 2500 {
        return Err(Cw2017Fail::Soc {
            soc,
            frac,
            vcell: raw,
            mode,
        });
    }
    Ok((soc, mv))
}

/// Voltage, temperature, and ID-pin raw value. The gauge has no current register.
pub fn cw2017_diag<I: I2c>(i2c: &mut I) -> heapless::String<96> {
    let mut s = heapless::String::new();
    let temp = cw_reg(i2c, 0x06).unwrap_or(0xFF);
    let idh = cw_reg(i2c, 0x0E).unwrap_or(0);
    let idl = cw_reg(i2c, 0x0F).unwrap_or(0);
    let vh = cw_reg(i2c, 0x02).unwrap_or(0);
    let vl = cw_reg(i2c, 0x03).unwrap_or(0);
    let vraw = (u16::from(vh) << 8 | u16::from(vl)) & 0x3FFF;
    let mv = (u32::from(vraw) * 3125 / 10_000) as u16;
    let id = (u16::from(idh) << 8 | u16::from(idl)) & 0x3FFF;
    let _ = core::fmt::Write::write_fmt(
        &mut s,
        format_args!("[bat] v={mv}mV temp={temp:02x} id={id:#x}"),
    );
    s
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
