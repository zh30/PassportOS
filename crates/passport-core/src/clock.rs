//! Local wall clock. No timezone and no NTP — firmware ticks seconds, USB sets HH:MM.

use core::fmt::Write;
use heapless::String;

use crate::api::Store;

/// Minutes-of-day, little-endian u16. Same KV page as Flap best.
pub const TIME_KEY: &[u8] = b"tod";

const DAY_SECS: u32 = 24 * 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Clock {
    /// Seconds since local midnight. `None` paints `--:--`.
    sod: Option<u32>,
}

impl Clock {
    pub const fn new() -> Self {
        Self { sod: None }
    }

    pub fn is_set(self) -> bool {
        self.sod.is_some()
    }

    pub fn set_hm(&mut self, h: u8, m: u8) -> Result<(), ()> {
        if h > 23 || m > 59 {
            return Err(());
        }
        self.sod = Some(u32::from(h) * 3600 + u32::from(m) * 60);
        Ok(())
    }

    pub fn set_minutes(&mut self, mins: u16) -> Result<(), ()> {
        if mins >= 24 * 60 {
            return Err(());
        }
        self.sod = Some(u32::from(mins) * 60);
        Ok(())
    }

    pub fn minutes_of_day(self) -> Option<u16> {
        self.sod.map(|s| ((s / 60) % (24 * 60)) as u16)
    }

    /// True when the displayed `HH:MM` changed.
    pub fn add_secs(&mut self, secs: u32) -> bool {
        let Some(sod) = self.sod.as_mut() else {
            return false;
        };
        let before = *sod / 60;
        *sod = (*sod + secs) % DAY_SECS;
        *sod / 60 != before
    }

    pub fn hour_minute(self) -> Option<(u8, u8)> {
        let s = self.sod?;
        Some(((s / 3600) as u8, ((s % 3600) / 60) as u8))
    }

    pub fn format_hm(self) -> String<5> {
        let mut s = String::new();
        match self.hour_minute() {
            Some((h, m)) => {
                let _ = write!(s, "{h:02}:{m:02}");
            }
            None => {
                let _ = s.push_str("--:--");
            }
        }
        s
    }
}

pub fn parse_hm(text: &str) -> Result<(u8, u8), ()> {
    let mut parts = text.split(':');
    let h: u8 = parts.next().ok_or(())?.parse().map_err(|_| ())?;
    let m: u8 = parts.next().ok_or(())?.parse().map_err(|_| ())?;
    if parts.next().is_some() || h > 23 || m > 59 {
        return Err(());
    }
    Ok((h, m))
}

pub fn read_tod(store: &dyn Store) -> Option<u16> {
    let mut buf = [0u8; 2];
    match store.get(TIME_KEY, &mut buf) {
        Some(2) => {
            let mins = u16::from_le_bytes(buf);
            if mins < 24 * 60 { Some(mins) } else { None }
        }
        _ => None,
    }
}

pub fn write_tod(store: &mut dyn Store, mins: u16) {
    let _ = store.put(TIME_KEY, &mins.to_le_bytes());
}
