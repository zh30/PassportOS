//! ES8311 register poke list. Firmware writes these over I2C; no HAL types here.
//!
//! Clock manager 0x01 must end with ADC digital + analog clocks on. A truncated
//! copy of ESP-ADF `es8311_open` left 0x01 at 0x30 (MCLK+BCLK only) and I2S
//! captured silence — Boo never heard a shout.

/// MCLK from pin, every codec clock on (incl. ADC digital bit3 + analog bit1).
pub const CLK_ALL_ON: u8 = 0x3F;
pub const CLKADC_ON: u8 = 1 << 3;
pub const ANACLKADC_ON: u8 = 1 << 1;
/// CSM on, I2S slave (bit6 clear).
pub const CSM_ON: u8 = 0x80;
/// Analog MIC1, PGA +30 dB. DMIC bit6 stays clear.
pub const MIC_ANALOG_PGA30: u8 = 0x1A;
/// GPIO 0x44: no DAC→ADC loopback (silent TX would otherwise look like silence).
pub const GPIO_NO_DAC_REF: u8 = 0x08;

/// Slave, 16 kHz-class, analog mic, ADC clocks on. Last write to a register wins.
pub const INIT: &[(u8, u8)] = &[
    (0x44, GPIO_NO_DAC_REF),
    (0x44, GPIO_NO_DAC_REF),
    (0x00, 0x1F),
    (0x00, 0x00),
    (0x00, CSM_ON),
    (0x01, CLK_ALL_ON),
    // 16 kHz, MCLK=256×fs: ESP-ADF coeff_div {pre_div=2, lrck=256, bclk_div=4}.
    (0x02, 0x20),
    (0x03, 0x10),
    (0x16, 0x24),
    (0x04, 0x10),
    (0x05, 0x00),
    (0x06, 0x03),
    (0x07, 0x00),
    (0x08, 0xFF),
    (0x0B, 0x00),
    (0x0C, 0x00),
    (0x10, 0x1F),
    (0x11, 0x7F),
    (0x09, 0x0C),
    (0x0A, 0x0C),
    (0x0D, 0x01),
    (0x0E, 0x02),
    (0x12, 0x00),
    (0x13, 0x10),
    (0x14, MIC_ANALOG_PGA30),
    (0x15, 0x40),
    (0x17, 0xBF),
    (0x1B, 0x0A),
    (0x1C, 0x6A),
    (0x32, 0xBF),
    (0x37, 0x08),
    (0x44, GPIO_NO_DAC_REF),
];

/// After I2S master clocks are running. Boot INIT happens before MCLK exists.
pub const START: &[(u8, u8)] = &[
    (0x00, CSM_ON),
    (0x01, CLK_ALL_ON),
    (0x02, 0x20),
    (0x03, 0x10),
    (0x04, 0x10),
    (0x05, 0x00),
    (0x06, 0x03),
    (0x07, 0x00),
    (0x08, 0xFF),
    (0x0D, 0x01),
    (0x0E, 0x02),
    (0x14, MIC_ANALOG_PGA30),
    (0x15, 0x40),
    (0x16, 0x24),
    (0x17, 0xBF),
    (0x0A, 0x0C),
    (0x45, 0x00),
];

pub fn last_write(reg: u8) -> Option<u8> {
    INIT.iter().rev().find(|(r, _)| *r == reg).map(|(_, v)| *v)
}

pub fn adc_clocks_on(reg01: u8) -> bool {
    (reg01 & CLKADC_ON) != 0 && (reg01 & ANACLKADC_ON) != 0
}
