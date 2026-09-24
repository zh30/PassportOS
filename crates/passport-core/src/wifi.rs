//! System Wi-Fi picker. Scan list + 3-key IME + saved creds. No HAL types.

use heapless::{String, Vec};

use crate::api::Store;
use crate::board::Key;
use crate::ime::{Ime, ImeAction};
use crate::input::ButtonEvent;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiAction {
    None,
    Scan,
    Connect,
    Disconnect,
    /// Clear saved creds; firmware also erases the KV record.
    Forget,
}

pub const WIFI_MAX_NETS: usize = 16;
pub const WIFI_VISIBLE: usize = 10;
pub const WIFI_SSID_MAX: usize = 32;
pub const WIFI_PASS_MAX: usize = 64;

/// KV keys persisted by the firmware flash store.
pub const WIFI_SSID_KEY: &[u8] = b"wifi.ssid";
pub const WIFI_PASS_KEY: &[u8] = b"wifi.pass";
pub const WIFI_OPEN_KEY: &[u8] = b"wifi.open";

/// Bounded open-network auto-rejoin after a drop (avoid a retry storm when
/// the AP is simply gone).
pub const REJOIN_MAX: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiPhase {
    Scan,
    List,
    Ime,
    Connecting,
    Result,
}

/// Why a join/association attempt ended. Firmware maps esp-radio errors here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiFail {
    Timeout,
    Auth,
    NoAp,
    Radio,
    Dropped,
}

impl WifiFail {
    pub const fn as_str(self) -> &'static str {
        match self {
            WifiFail::Timeout => "timeout",
            WifiFail::Auth => "auth",
            WifiFail::NoAp => "no-ap",
            WifiFail::Radio => "radio",
            WifiFail::Dropped => "dropped",
        }
    }
}

/// Saved credentials: one network only (4 KB KV page, one record).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedWifi {
    pub ssid: String<WIFI_SSID_MAX>,
    pub pass: String<WIFI_PASS_MAX>,
    pub open: bool,
}

impl SavedWifi {
    pub fn new(ssid: &str, pass: &str, open: bool) -> Option<Self> {
        if ssid.is_empty() {
            return None;
        }
        let mut s = String::new();
        s.push_str(ssid).ok()?;
        let mut p = String::new();
        p.push_str(pass).ok()?;
        Some(Self {
            ssid: s,
            pass: p,
            open,
        })
    }

    pub fn load(store: &dyn Store) -> Option<Self> {
        let mut ssid = [0u8; WIFI_SSID_MAX];
        let n = store.get(WIFI_SSID_KEY, &mut ssid)?;
        if n == 0 || n > WIFI_SSID_MAX {
            return None;
        }
        let ssid = core::str::from_utf8(&ssid[..n]).ok()?;
        let mut pass = [0u8; WIFI_PASS_MAX];
        let pass_str = match store.get(WIFI_PASS_KEY, &mut pass) {
            Some(m) if m <= WIFI_PASS_MAX => core::str::from_utf8(&pass[..m]).ok()?,
            _ => "",
        };
        let mut open = [0u8; 1];
        let open = store.get(WIFI_OPEN_KEY, &mut open) == Some(1) && open[0] == 1;
        Self::new(ssid, pass_str, open)
    }

    pub fn save(&self, store: &mut dyn Store) {
        let _ = store.put(WIFI_SSID_KEY, self.ssid.as_bytes());
        let _ = store.put(WIFI_PASS_KEY, self.pass.as_bytes());
        let _ = store.put(WIFI_OPEN_KEY, &[self.open as u8]);
    }

    pub fn erase(store: &mut dyn Store) {
        let _ = store.put(WIFI_SSID_KEY, &[]);
        let _ = store.put(WIFI_PASS_KEY, &[]);
        let _ = store.put(WIFI_OPEN_KEY, &[]);
    }
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

/// A footer row appended after the scan list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiRow {
    Net(usize),
    Rescan,
    Disconnect,
    Forget,
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
    fail: Option<WifiFail>,
    connected_ssid: String<WIFI_SSID_MAX>,
    saved: Option<SavedWifi>,
    /// Open-network rejoin attempts since the last successful connect.
    rejoins: u8,
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
            fail: None,
            connected_ssid: String::new(),
            saved: None,
            rejoins: 0,
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

    /// Why the last join failed (or `Dropped` after a lost association).
    pub fn fail(&self) -> Option<WifiFail> {
        self.fail
    }

    pub fn saved(&self) -> Option<&SavedWifi> {
        self.saved.as_ref()
    }

    /// Boot-time restore from the KV page.
    pub fn load_saved(&mut self, store: &dyn Store) {
        self.saved = SavedWifi::load(store);
        self.rejoins = 0;
    }

    /// Persisted creds after a successful join / direct `wifi join`.
    pub fn saved_pending(&self) -> Option<SavedWifi> {
        SavedWifi::new(
            self.pending_ssid.as_str(),
            self.pending_pass.as_str(),
            self.pending_open,
        )
    }

    /// Commit `pending` as the saved entry. Firmware calls this once the KV
    /// write is queued (the Store put itself is infallible in RAM).
    pub fn commit_saved(&mut self, store: &mut dyn Store) {
        if let Some(s) = self.saved_pending() {
            s.save(store);
            self.saved = Some(s);
        }
    }

    /// `wifi join <ssid> [pass]` — skip scan/IME entirely.
    pub fn join(&mut self, ssid: &str, pass: &str, open: bool) -> WifiAction {
        if ssid.is_empty() || ssid.len() > WIFI_SSID_MAX || pass.len() > WIFI_PASS_MAX {
            return WifiAction::None;
        }
        self.pending_ssid.clear();
        let _ = self.pending_ssid.push_str(ssid);
        self.pending_pass.clear();
        let _ = self.pending_pass.push_str(pass);
        self.pending_open = open;
        self.rejoins = 0;
        self.phase = WifiPhase::Connecting;
        WifiAction::Connect
    }

    /// Association lost while connected. Clears the connected mark; for an
    /// open saved network, returns `Connect` for a bounded auto-rejoin.
    /// `rejoins` counts automatic rejoin attempts since the last user connect
    /// so a flapping AP cannot loop forever.
    pub fn note_drop(&mut self) -> WifiAction {
        let was_connected = !self.connected_ssid.is_empty();
        self.connected_ssid.clear();
        if self.phase == WifiPhase::Connecting {
            // Connect raced a drop — surface it as a failed join.
            self.fail = Some(WifiFail::Dropped);
            self.result_ok = false;
            self.phase = WifiPhase::Result;
            return WifiAction::None;
        }
        if was_connected {
            self.fail = Some(WifiFail::Dropped);
        }
        if self.saved.as_ref().is_some_and(|s| s.open) && self.rejoins < REJOIN_MAX {
            self.rejoins += 1;
            if let Some(s) = self.saved.clone() {
                self.pending_ssid = s.ssid;
                self.pending_pass = s.pass;
                self.pending_open = s.open;
                self.phase = WifiPhase::Connecting;
                return WifiAction::Connect;
            }
        }
        WifiAction::None
    }

    /// User asked to drop the link but keep creds (console/menu row).
    pub fn disconnect(&mut self) -> WifiAction {
        self.connected_ssid.clear();
        self.rejoins = REJOIN_MAX; // explicit drop: no auto-rejoin
        WifiAction::Disconnect
    }

    /// Forget saved creds (RAM side; firmware erases KV + disconnects).
    pub fn forget(&mut self) -> WifiAction {
        self.saved = None;
        self.connected_ssid.clear();
        self.rejoins = REJOIN_MAX;
        WifiAction::Forget
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

    /// Nets, a `rescan` row, then `disconnect`+`forget` while associated
    /// (or `forget` alone when only saved creds exist).
    pub fn row_count(&self) -> usize {
        self.nets.len() + 1 + self.extra_rows()
    }

    fn extra_rows(&self) -> usize {
        if !self.connected_ssid.is_empty() {
            2
        } else if self.saved.is_some() {
            1
        } else {
            0
        }
    }

    pub fn row_kind(&self, idx: usize) -> Option<WifiRow> {
        let n = self.nets.len();
        if idx < n {
            return Some(WifiRow::Net(idx));
        }
        match idx - n {
            0 => Some(WifiRow::Rescan),
            1 if !self.connected_ssid.is_empty() => Some(WifiRow::Disconnect),
            1 if self.saved.is_some() => Some(WifiRow::Forget),
            2 if !self.connected_ssid.is_empty() => Some(WifiRow::Forget),
            _ => None,
        }
    }

    pub fn row_is_scan(&self, idx: usize) -> bool {
        self.row_kind(idx) == Some(WifiRow::Rescan)
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
        self.fail = None;
        self.rejoins = 0;
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

    pub fn apply_result(&mut self, ok: bool, fail: Option<WifiFail>) {
        self.result_ok = ok;
        self.fail = fail;
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
        match self.row_kind(self.sel) {
            Some(WifiRow::Rescan) => {
                self.begin_scan();
                WifiAction::Scan
            }
            Some(WifiRow::Disconnect) => self.disconnect(),
            Some(WifiRow::Forget) => self.forget(),
            Some(WifiRow::Net(_)) => self.activate_net(),
            None => WifiAction::None,
        }
    }

    fn activate_net(&mut self) -> WifiAction {
        let Some(net) = self.nets.get(self.sel) else {
            return WifiAction::None;
        };
        let ssid = net.ssid.clone();
        let open = net.open;
        self.rejoins = 0;
        // A saved entry for this SSID means we already hold the password:
        // skip the IME unless the scan says the network is open.
        if let Some(saved) = &self.saved {
            if saved.ssid == ssid && (!open || saved.open) {
                self.pending_ssid = saved.ssid.clone();
                self.pending_pass = saved.pass.clone();
                self.pending_open = saved.open;
                self.phase = WifiPhase::Connecting;
                return WifiAction::Connect;
            }
        }
        self.pending_ssid = ssid;
        self.pending_open = open;
        self.pending_pass.clear();
        if open {
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
