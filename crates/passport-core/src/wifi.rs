//! System Wi-Fi picker. Scan list + 3-key IME. No HAL types.

use heapless::{String, Vec};

use crate::board::Key;
use crate::ime::{Ime, ImeAction};
use crate::input::ButtonEvent;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiAction {
    None,
    Scan,
    Connect,
}

pub const WIFI_MAX_NETS: usize = 16;
pub const WIFI_VISIBLE: usize = 10;
pub const WIFI_SSID_MAX: usize = 32;
pub const WIFI_PASS_MAX: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiPhase {
    Scan,
    List,
    Ime,
    Connecting,
    Result,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WifiNet {
    pub ssid: String<WIFI_SSID_MAX>,
    pub open: bool,
    pub rssi: i8,
}

impl WifiNet {
    pub fn new(ssid: &str, open: bool, rssi: i8) -> Result<Self, ()> {
        if ssid.is_empty() {
            return Err(());
        }
        let mut s = String::new();
        s.push_str(ssid).map_err(|_| ())?;
        Ok(Self {
            ssid: s,
            open,
            rssi,
        })
    }
}

#[derive(Clone, Debug)]
pub struct WifiUi {
    phase: WifiPhase,
    nets: Vec<WifiNet, WIFI_MAX_NETS>,
    sel: usize,
    ime: Ime,
    pending_ssid: String<WIFI_SSID_MAX>,
    pending_pass: String<WIFI_PASS_MAX>,
    pending_open: bool,
    result_ok: bool,
    connected_ssid: String<WIFI_SSID_MAX>,
}

impl Default for WifiUi {
    fn default() -> Self {
        Self::new()
    }
}

impl WifiUi {
    pub const fn new() -> Self {
        Self {
            phase: WifiPhase::Scan,
            nets: Vec::new(),
            sel: 0,
            ime: Ime::new(),
            pending_ssid: String::new(),
            pending_pass: String::new(),
            pending_open: false,
            result_ok: false,
            connected_ssid: String::new(),
        }
    }

    pub fn phase(&self) -> WifiPhase {
        self.phase
    }

    pub fn nets(&self) -> &[WifiNet] {
        &self.nets
    }

    pub fn selected(&self) -> usize {
        self.sel
    }

    pub fn ime(&self) -> &Ime {
        &self.ime
    }

    pub fn result_ok(&self) -> bool {
        self.result_ok
    }

    pub fn pending_ssid(&self) -> &str {
        self.pending_ssid.as_str()
    }

    pub fn pending_pass(&self) -> &str {
        self.pending_pass.as_str()
    }

    pub fn pending_open(&self) -> bool {
        self.pending_open
    }

    pub fn connected_ssid(&self) -> &str {
        self.connected_ssid.as_str()
    }

    /// Nets plus a trailing `scan` row.
    pub fn row_count(&self) -> usize {
        self.nets.len() + 1
    }

    pub fn row_is_scan(&self, idx: usize) -> bool {
        idx == self.nets.len()
    }

    pub fn net_at(&self, idx: usize) -> Option<&WifiNet> {
        self.nets.get(idx)
    }

    /// Visible slice `[start, start+len)` around the selection.
    pub fn window(&self, max: usize) -> (usize, usize) {
        let n = self.row_count();
        let max = max.max(1);
        if n <= max {
            return (0, n);
        }
        let mut start = self.sel.saturating_sub(max / 2);
        if start + max > n {
            start = n - max;
        }
        (start, max)
    }

    pub fn begin_scan(&mut self) {
        self.phase = WifiPhase::Scan;
        self.nets.clear();
        self.sel = 0;
        self.ime.reset();
        self.pending_ssid.clear();
        self.pending_pass.clear();
        self.pending_open = false;
    }

    pub fn apply_scan(&mut self, nets: &[WifiNet]) {
        self.nets.clear();
        for n in nets {
            self.push_dedup(n);
        }
        self.nets
            .sort_unstable_by(|a, b| b.rssi.cmp(&a.rssi).then_with(|| a.ssid.cmp(&b.ssid)));
        self.sel = 0;
        if self.phase == WifiPhase::Scan {
            self.phase = WifiPhase::List;
        }
    }

    fn push_dedup(&mut self, net: &WifiNet) {
        if net.ssid.is_empty() {
            return;
        }
        if let Some(existing) = self.nets.iter_mut().find(|n| n.ssid == net.ssid) {
            if net.rssi > existing.rssi {
                existing.rssi = net.rssi;
                existing.open = net.open;
            }
            return;
        }
        let _ = self.nets.push(net.clone());
    }

    pub fn apply_result(&mut self, ok: bool) {
        self.result_ok = ok;
        self.phase = WifiPhase::Result;
        if ok {
            self.connected_ssid.clear();
            let _ = self.connected_ssid.push_str(self.pending_ssid.as_str());
        }
    }

    pub fn handle(&mut self, ev: ButtonEvent) -> WifiAction {
        match ev {
            ButtonEvent::Click(Key::Up) => self.move_sel(-1),
            ButtonEvent::Click(Key::Down) => self.move_sel(1),
            ButtonEvent::Click(Key::Ok) => self.activate(),
            _ => WifiAction::None,
        }
    }

    fn move_sel(&mut self, delta: i16) -> WifiAction {
        match self.phase {
            WifiPhase::List => {
                let n = self.row_count() as i16;
                if n == 0 {
                    return WifiAction::None;
                }
                let mut s = self.sel as i16 + delta;
                s = ((s % n) + n) % n;
                self.sel = s as usize;
            }
            WifiPhase::Ime => self.ime.move_sel(delta),
            WifiPhase::Scan | WifiPhase::Connecting | WifiPhase::Result => {}
        }
        WifiAction::None
    }

    fn activate(&mut self) -> WifiAction {
        match self.phase {
            WifiPhase::List => self.activate_list(),
            WifiPhase::Ime => self.activate_ime(),
            WifiPhase::Result => {
                self.phase = WifiPhase::List;
                WifiAction::None
            }
            WifiPhase::Scan | WifiPhase::Connecting => WifiAction::None,
        }
    }

    fn activate_list(&mut self) -> WifiAction {
        if self.row_is_scan(self.sel) {
            self.begin_scan();
            return WifiAction::Scan;
        }
        let Some(net) = self.nets.get(self.sel) else {
            return WifiAction::None;
        };
        self.pending_ssid.clear();
        let _ = self.pending_ssid.push_str(net.ssid.as_str());
        self.pending_open = net.open;
        self.pending_pass.clear();
        if net.open {
            self.phase = WifiPhase::Connecting;
            WifiAction::Connect
        } else {
            self.ime.reset();
            self.phase = WifiPhase::Ime;
            WifiAction::None
        }
    }

    fn activate_ime(&mut self) -> WifiAction {
        match self.ime.click() {
            ImeAction::Done => {
                if self.ime.is_empty() && !self.pending_open {
                    return WifiAction::None;
                }
                self.pending_pass.clear();
                let _ = self.pending_pass.push_str(self.ime.buffer());
                self.phase = WifiPhase::Connecting;
                WifiAction::Connect
            }
            ImeAction::Cancel => {
                self.ime.reset();
                self.phase = WifiPhase::List;
                WifiAction::None
            }
            ImeAction::Edit | ImeAction::None => WifiAction::None,
        }
    }
}
