//! System microphone: 8-bit level in, edge events out. No I2S types.

/// Rising edge through [`ROAR_LEVEL`] (or the calibrated shout floor).
pub const ROAR_LEVEL: u8 = 48;
/// Rising edge through [`LOUD_LEVEL`] (or the calibrated peak).
pub const LOUD_LEVEL: u8 = 200;

const CALIBRATE_N: u8 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicEvent {
    /// Crossed the shout threshold. Holding a yell is one event, not a stream.
    Loud,
    /// Crossed the peak (too-loud) threshold.
    Peak,
}

/// Edge-trigger a shout. Same idea as [`crate::input::ButtonDecoder`]: level is
/// analog, the event is a press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MicGate {
    high: bool,
}

impl Default for MicGate {
    fn default() -> Self {
        Self::new()
    }
}

impl MicGate {
    pub const fn new() -> Self {
        Self { high: false }
    }

    pub fn feed(&mut self, level: u8, roar_at: u8, loud_at: u8) -> Option<MicEvent> {
        if level >= loud_at {
            if self.high {
                return None;
            }
            self.high = true;
            return Some(MicEvent::Peak);
        }
        if level >= roar_at {
            if self.high {
                return None;
            }
            self.high = true;
            return Some(MicEvent::Loud);
        }
        self.high = false;
        None
    }
}

/// Live mic decoder owned by [`crate::shell::Shell`].
#[derive(Clone, Copy, Debug)]
pub struct Mic {
    gate: MicGate,
    level: u8,
    roar_at: u8,
    loud_at: u8,
    noise_acc: u16,
    noise_n: u8,
    pcm_peak: u16,
}

impl Default for Mic {
    fn default() -> Self {
        Self::new()
    }
}

impl Mic {
    pub const fn new() -> Self {
        Self {
            gate: MicGate::new(),
            level: 0,
            roar_at: ROAR_LEVEL,
            loud_at: LOUD_LEVEL,
            noise_acc: 0,
            noise_n: 0,
            pcm_peak: 0,
        }
    }

    pub const fn level(&self) -> u8 {
        self.level
    }

    pub const fn roar_at(&self) -> u8 {
        self.roar_at
    }

    pub const fn loud_at(&self) -> u8 {
        self.loud_at
    }

    pub const fn pcm_peak(&self) -> u16 {
        self.pcm_peak
    }

    pub fn note_peak(&mut self, peak: u16) {
        if peak > self.pcm_peak {
            self.pcm_peak = peak;
        }
    }

    /// Reset edge state and start a short noise-floor calibration.
    pub fn begin(&mut self) {
        *self = Self::new();
    }

    pub fn feed(&mut self, level: u8) -> Option<MicEvent> {
        self.level = level;
        self.calibrate(level);
        self.gate.feed(level, self.roar_at, self.loud_at)
    }

    fn calibrate(&mut self, level: u8) {
        if self.noise_n >= CALIBRATE_N {
            return;
        }
        self.noise_acc = self.noise_acc.saturating_add(u16::from(level));
        self.noise_n = self.noise_n.saturating_add(1);
        if self.noise_n >= CALIBRATE_N {
            let avg = (self.noise_acc / u16::from(self.noise_n)) as u8;
            self.roar_at = avg.saturating_add(24).clamp(32, 100);
            self.loud_at = self.roar_at.saturating_add(72).clamp(160, 250);
        }
    }
}

/// Max absolute 16-bit sample. Distinguishes "DMA zeros" from "quiet room".
pub fn pcm16_le_peak(bytes: &[u8]) -> u16 {
    let mut peak = 0u16;
    for c in bytes.chunks_exact(2) {
        let s = i16::from_le_bytes([c[0], c[1]]).unsigned_abs();
        if s > peak {
            peak = s;
        }
    }
    peak
}

/// 16-bit little-endian PCM → 0..=255 RMS. Firmware calls this; tests do too.
pub fn pcm16_le_level(bytes: &[u8]) -> u8 {
    let mut acc = 0u64;
    let mut n = 0u32;
    for c in bytes.chunks_exact(2) {
        let s = i16::from_le_bytes([c[0], c[1]]) as i32;
        acc = acc.saturating_add((s * s) as u64);
        n = n.saturating_add(1);
    }
    if n == 0 {
        return 0;
    }
    let rms = isqrt(acc / u64::from(n));
    (rms / 128).min(255) as u8
}

fn isqrt(n: u64) -> u32 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = x.saturating_add(1) / 2;
    while y < x {
        x = y;
        y = x.saturating_add(n / x) / 2;
    }
    x as u32
}
