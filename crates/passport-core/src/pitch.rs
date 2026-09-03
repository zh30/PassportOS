//! Pitch from 16-bit PCM. Firmware feeds I2S bytes; no HAL types.

/// Matches firmware I2S (`Rate::from_hz(16_000)`).
pub const PITCH_RATE: u32 = 16_000;
pub const PITCH_N: usize = 512;
pub const PITCH_MIN_HZ: u16 = 70;
pub const PITCH_MAX_HZ: u16 = 900;
/// Mean-abs below this is silence (keeps AMDF off the noise floor).
const ENERGY_MIN: u32 = 180;

#[derive(Clone, Copy, Debug)]
pub struct PitchBuf {
    samples: [i16; PITCH_N],
    w: usize,
    n: usize,
}

impl Default for PitchBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl PitchBuf {
    pub const fn new() -> Self {
        Self {
            samples: [0; PITCH_N],
            w: 0,
            n: 0,
        }
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }

    pub fn len(&self) -> usize {
        self.n
    }

    /// Left channel of 16-bit LE stereo (L,R,L,R…).
    pub fn push_pcm16_le_left(&mut self, bytes: &[u8]) {
        for c in bytes.chunks_exact(4) {
            let s = i16::from_le_bytes([c[0], c[1]]);
            self.samples[self.w] = s;
            self.w += 1;
            if self.w == PITCH_N {
                self.w = 0;
            }
            if self.n < PITCH_N {
                self.n += 1;
            }
        }
    }

    pub fn copy_linear(&self, out: &mut [i16; PITCH_N]) -> usize {
        if self.n < PITCH_N {
            out[..self.n].copy_from_slice(&self.samples[..self.n]);
            self.n
        } else {
            let (a, b) = self.samples.split_at(self.w);
            let k = b.len();
            out[..k].copy_from_slice(b);
            out[k..].copy_from_slice(a);
            PITCH_N
        }
    }

    pub fn hz(&self) -> Option<u16> {
        let mut tmp = [0i16; PITCH_N];
        let n = self.copy_linear(&mut tmp);
        amdf_hz(&tmp[..n], PITCH_RATE, PITCH_MIN_HZ, PITCH_MAX_HZ)
    }

    pub fn hz_near(&self, target: u16) -> Option<u16> {
        let lo = target.saturating_mul(85) / 100;
        let hi = target.saturating_mul(115) / 100;
        let mut tmp = [0i16; PITCH_N];
        let n = self.copy_linear(&mut tmp);
        amdf_hz(
            &tmp[..n],
            PITCH_RATE,
            lo.max(PITCH_MIN_HZ),
            hi.min(PITCH_MAX_HZ).max(lo + 1),
        )
    }
}

/// Average magnitude difference. `lo_hz..=hi_hz` is the search window.
pub fn amdf_hz(samples: &[i16], rate: u32, lo_hz: u16, hi_hz: u16) -> Option<u16> {
    if samples.len() < 64 || rate == 0 || lo_hz == 0 || hi_hz <= lo_hz {
        return None;
    }
    let mut energy = 0u64;
    for s in samples {
        energy += s.unsigned_abs() as u64;
    }
    if (energy / samples.len() as u64) < u64::from(ENERGY_MIN) {
        return None;
    }
    let max_p = (rate / u32::from(lo_hz))
        .min(samples.len() as u32 / 2)
        .max(3);
    let min_p = (rate / u32::from(hi_hz)).max(2);
    if min_p >= max_p {
        return None;
    }
    let mut best_p = min_p;
    let mut best = i64::MAX;
    let mut p = min_p;
    while p <= max_p {
        let n = samples.len() - p as usize;
        let mut cost = 0i64;
        let mut i = 0;
        while i < n {
            cost += (samples[i] as i32 - samples[i + p as usize] as i32).unsigned_abs() as i64;
            i += 2;
        }
        cost /= (n / 2).max(1) as i64;
        if cost < best {
            best = cost;
            best_p = p;
        }
        p += 1;
    }
    Some(((rate + best_p / 2) / best_p) as u16)
}

/// First-order cents: 1200/ln(2) ≈ 1731. Tight around the target (tuner range).
pub fn cents(hz: u16, target: u16) -> i16 {
    if target == 0 || hz == 0 {
        return 0;
    }
    let d = i32::from(hz) - i32::from(target);
    (d * 1731 / i32::from(target)).clamp(-120, 120) as i16
}

/// Fold `hz` into the octave around `target` so AMDF 2×/½ errors still tune.
pub fn fold_octave(mut hz: u16, target: u16) -> u16 {
    if hz == 0 || target == 0 {
        return hz;
    }
    while hz >= target.saturating_mul(3) / 2 {
        hz /= 2;
        if hz == 0 {
            return 0;
        }
    }
    while hz.saturating_mul(3) / 2 < target {
        hz = hz.saturating_mul(2);
    }
    hz
}
