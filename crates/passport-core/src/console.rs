//! USB console grammar. Key inject goes through the millivolt decoder, not a side path.

use crate::board::Key;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Status,
    Apps,
    Launcher,
    Menu,
    Keys,
    About,
    Workspace(u8),
    Filter(heapless::String<16>),
    ActivateSelected,
    ActivateName(heapless::String<16>),
    /// Single millivolt sample, `dt_ms` ticks of 20 ms (same path as ADC).
    KeyMv { mv: u16, ticks: u8 },
    KeyClick(Key),
    KeyLong(Key),
    Brightness(u8),
    RadioWifi,
    RadioBle,
    RadioOff,
    SleepLight,
    SleepDeep,
    Probe,
    AudioBeep,
    AudioRec,
    Nfc,
    Theme(crate::theme::Theme),
    ThemeToggle,
    /// `None` prints the clock; `Some` sets local HH:MM.
    Time(Option<(u8, u8)>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    Unknown,
    BadArg,
}

pub fn parse_line(line: &str) -> Result<Command, ParseError> {
    let line = line.trim();
    if line.is_empty() {
        return Err(ParseError::Empty);
    }
    let mut parts = line.split_whitespace();
    let cmd = parts.next().ok_or(ParseError::Empty)?;
    match cmd {
        "help" | "?" => Ok(Command::Help),
        "status" => Ok(Command::Status),
        "apps" => Ok(Command::Apps),
        "launcher" => Ok(Command::Launcher),
        "menu" => Ok(Command::Menu),
        "keys" => Ok(Command::Keys),
        "about" => Ok(Command::About),
        "time" => match parts.next() {
            None => Ok(Command::Time(None)),
            Some(hm) => {
                let (h, m) = crate::clock::parse_hm(hm).map_err(|_| ParseError::BadArg)?;
                Ok(Command::Time(Some((h, m))))
            }
        },
        "theme" => match parts.next() {
            Some("light") => Ok(Command::Theme(crate::theme::Theme::Light)),
            Some("dark") => Ok(Command::Theme(crate::theme::Theme::Dark)),
            Some("toggle") | None => Ok(Command::ThemeToggle),
            _ => Err(ParseError::BadArg),
        },
        "probe" => Ok(Command::Probe),
        "nfc" | "ntag" => Ok(Command::Nfc),
        "filter" => {
            let p = parts.next().unwrap_or("");
            let mut s = heapless::String::new();
            s.push_str(p).map_err(|_| ParseError::BadArg)?;
            Ok(Command::Filter(s))
        }
        "workspace" | "ws" => {
            let n: u8 = parts.next().ok_or(ParseError::BadArg)?.parse().map_err(|_| ParseError::BadArg)?;
            if n >= 2 {
                return Err(ParseError::BadArg);
            }
            Ok(Command::Workspace(n))
        }
        "activate" => match parts.next() {
            None => Ok(Command::ActivateSelected),
            Some(name) => {
                let mut s = heapless::String::new();
                s.push_str(name).map_err(|_| ParseError::BadArg)?;
                Ok(Command::ActivateName(s))
            }
        },
        "key" => parse_key(parts),
        "brightness" | "bl" => {
            let n: u8 = parts.next().ok_or(ParseError::BadArg)?.parse().map_err(|_| ParseError::BadArg)?;
            Ok(Command::Brightness(n.min(100)))
        }
        "radio" => match parts.next() {
            Some("wifi") | Some("scan") => Ok(Command::RadioWifi),
            Some("ble") | Some("adv") => Ok(Command::RadioBle),
            Some("off") => Ok(Command::RadioOff),
            _ => Err(ParseError::BadArg),
        },
        "sleep" => match parts.next() {
            Some("light") => Ok(Command::SleepLight),
            Some("deep") => Ok(Command::SleepDeep),
            _ => Err(ParseError::BadArg),
        },
        "audio" => match parts.next() {
            Some("beep") | Some("play") => Ok(Command::AudioBeep),
            Some("rec") | Some("mic") => Ok(Command::AudioRec),
            _ => Err(ParseError::BadArg),
        },
        _ => Err(ParseError::Unknown),
    }
}

fn parse_key<'a>(mut parts: impl Iterator<Item = &'a str>) -> Result<Command, ParseError> {
    match parts.next() {
        Some("mv") => {
            let mv: u16 = parts.next().ok_or(ParseError::BadArg)?.parse().map_err(|_| ParseError::BadArg)?;
            let ticks: u8 = parts
                .next()
                .map(|t| t.parse().unwrap_or(1))
                .unwrap_or(1);
            Ok(Command::KeyMv { mv, ticks: ticks.max(1) })
        }
        Some(name) => {
            let key = match name {
                "up" => Key::Up,
                "down" | "dn" => Key::Down,
                "ok" | "enter" => Key::Ok,
                _ => return Err(ParseError::BadArg),
            };
            match parts.next() {
                Some("click") | Some("tap") | None => Ok(Command::KeyClick(key)),
                Some("long") | Some("hold") => Ok(Command::KeyLong(key)),
                _ => Err(ParseError::BadArg),
            }
        }
        None => Err(ParseError::BadArg),
    }
}

pub const HELP: &str = "\
help status apps launcher menu keys about probe nfc
key mv <mV> [ticks] | key up|down|ok click|long
workspace 0|1  filter <prefix>  activate [name]
brightness 0-100  radio wifi|ble|off
sleep light|deep  audio beep|rec  theme dark|light
time [HH:MM]
";
