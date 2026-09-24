//! Status bar fields: battery + radio + workspace + focused app.

use core::fmt::Write;
use heapless::String;

use crate::clock::Clock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadioMode {
    Off,
    Wifi,
    Ble,
}

impl RadioMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            RadioMode::Off => "off",
            RadioMode::Wifi => "wifi",
            RadioMode::Ble => "ble",
        }
    }
}

#[derive(Clone, Debug)]
pub struct StatusBar {
    pub battery_soc: Option<u8>,
    pub battery_mv: Option<u16>,
    /// USB-C 5 V present (SOF on the Serial/JTAG bus) or SOC rising.
    pub charging: bool,
    pub radio: RadioMode,
    /// A BLE central is connected right now (advertising alone is not enough).
    pub ble_conn: bool,
    /// Speaker output level 0..=100 applied to the ES8311 DAC.
    pub volume: u8,
    /// DAC muted. Kept separately so unmute restores `volume`.
    pub muted: bool,
    pub workspace: u8,
    pub focused: String<16>,
    pub brightness: u8,
    pub overlay: String<12>,
    pub clock: Clock,
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusBar {
    pub fn new() -> Self {
        let mut focused = String::new();
        let _ = focused.push_str("shell");
        Self {
            battery_soc: None,
            battery_mv: None,
            charging: false,
            radio: RadioMode::Off,
            ble_conn: false,
            volume: 80,
            muted: false,
            workspace: 0,
            focused,
            brightness: 80,
            overlay: String::new(),
            clock: Clock::new(),
        }
    }

    pub fn set_focused(&mut self, name: &str) {
        self.focused.clear();
        let _ = self.focused.push_str(name);
    }

    /// Serial status line. Always includes battery and radio fields.
    pub fn format(&self) -> String<96> {
        let mut s = String::new();
        let _ = write!(
            s,
            "[status] time={} bat={}{} radio={} ws={} app={} bl={}",
            self.clock.format_hm(),
            BatFmt(self.battery_soc, self.battery_mv),
            if self.charging { " chg" } else { "" },
            self.radio.as_str(),
            self.workspace,
            self.focused.as_str(),
            self.brightness
        );
        if self.ble_conn {
            let _ = s.push_str(" ble-conn");
        }
        if self.muted {
            let _ = s.push_str(" vol=mute");
        } else {
            let _ = write!(s, " vol={}", self.volume);
        }
        if !self.overlay.is_empty() {
            let _ = write!(s, " overlay={}", self.overlay.as_str());
        }
        s
    }

    /// 240 px / 6 px font = 40 columns. Keep battery + radio visible on the LCD.
    pub fn format_screen(&self) -> String<40> {
        let mut s = String::new();
        let bat = match (self.battery_soc, self.charging) {
            (Some(n), true) => {
                let mut b = String::<8>::new();
                let _ = write!(b, "{n}%+");
                b
            }
            (Some(n), false) => {
                let mut b = String::<8>::new();
                let _ = write!(b, "{n}%");
                b
            }
            (None, true) => {
                let mut b = String::<8>::new();
                let _ = b.push_str("--+");
                b
            }
            (None, false) => {
                let mut b = String::<8>::new();
                let _ = b.push_str("--");
                b
            }
        };
        let time = self.clock.format_hm();
        // "ble*" marks a live central connection, "ble" is advertising only.
        let radio = if self.radio == RadioMode::Ble && self.ble_conn {
            "ble*"
        } else {
            self.radio.as_str()
        };
        let _ = write!(
            s,
            "{time} {bat} {radio} ws{ws} {app}",
            time = time.as_str(),
            bat = bat.as_str(),
            radio = radio,
            ws = self.workspace + 1,
            app = self.focused.as_str()
        );
        s
    }
}

/// USB SOF changing means a host is on the Type-C bus (this board's 5 V input).
/// SOC increasing covers charge-only cables that carry no USB data.
pub const fn charging_from_samples(
    usb_sof: u16,
    last_usb_sof: u16,
    soc: Option<u8>,
    last_soc: Option<u8>,
) -> bool {
    let usb_live = usb_sof != 0 && usb_sof != last_usb_sof;
    let soc_up = match (soc, last_soc) {
        (Some(now), Some(prev)) => now > prev,
        _ => false,
    };
    usb_live || soc_up
}

struct BatFmt(Option<u8>, Option<u16>);

impl core::fmt::Display for BatFmt {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match (self.0, self.1) {
            (Some(soc), Some(mv)) => write!(f, "{soc}%({mv}mV)"),
            (Some(soc), None) => write!(f, "{soc}%"),
            (None, Some(mv)) => write!(f, "--({mv}mV)"),
            (None, None) => f.write_str("--"),
        }
    }
}
