//! Omarchy-like shell: status bar, launcher, tiles, workspaces, system menu.
//!
//! Button events — including USB console key inject — all pass through [`ButtonDecoder`].

use core::fmt::Write;
use heapless::{String, Vec};

use crate::app::{AppId, AppLifecycle, AppRegistry};
use crate::board::{decode_millivolts, Key, KeyState, TYPICAL_RELEASED_MV};
use crate::compositor::{layout_tiles, TileLayout, TILES_PER_WORKSPACE, WORKSPACE_COUNT};
use crate::console::{Command, HELP};
use crate::input::{ButtonDecoder, ButtonEvent, LONG_PRESS_MS};
use crate::launcher::{Launcher, LauncherKind};
use crate::menu::{MenuAction, SystemMenu};
use crate::radio::ExclusiveManager;
use crate::status::{RadioMode, StatusBar};
use crate::theme::Theme;
use crate::wifi::{WifiAction, WifiNet, WifiUi};

const TICK_MS: u32 = 20;
/// Unplugged idle before backlight-off standby. Not RTC `sleep light` / `sleep deep`.
pub const IDLE_STANDBY_MS: u32 = 30_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    None,
    Launcher,
    System,
    Keys,
    About,
    Wifi,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideEffect {
    None,
    WifiScan,
    WifiConnect,
    BleAdvertise,
    RadioOff,
    SleepLight,
    SleepDeep,
    AudioBeep,
    AudioRec,
    Probe,
    SetBrightness(u8),
    ClockSet,
}

#[derive(Clone, Debug)]
pub struct CommandOutcome {
    pub lifecycle: Vec<AppLifecycle, 8>,
    pub reply: String<256>,
    pub side: SideEffect,
}

/// Result of a button / millivolt event. Firmware applies `side` the same way as
/// [`CommandOutcome::side`] from the USB console.
#[derive(Clone, Debug)]
pub struct EventOutcome {
    pub lifecycle: Vec<AppLifecycle, 8>,
    pub side: SideEffect,
}

impl EventOutcome {
    pub fn empty() -> Self {
        Self {
            lifecycle: Vec::new(),
            side: SideEffect::None,
        }
    }

    pub fn from_lifecycle(lifecycle: Vec<AppLifecycle, 8>) -> Self {
        Self {
            lifecycle,
            side: SideEffect::None,
        }
    }
}

fn merge_side(dst: &mut SideEffect, src: SideEffect) {
    if src != SideEffect::None {
        *dst = src;
    }
}

#[derive(Clone, Debug)]
struct Workspace {
    /// Occupied tile slots (app ids), max two, non-overlapping tiles.
    slots: Vec<AppId, TILES_PER_WORKSPACE>,
    focused: usize,
}

impl Workspace {
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            focused: 0,
        }
    }

    fn tile_count(&self) -> usize {
        self.slots.len()
    }

    fn focused_id(&self) -> Option<AppId> {
        self.slots.get(self.focused).copied()
    }
}

pub struct Shell {
    decoder: ButtonDecoder,
    pub registry: AppRegistry,
    workspaces: [Workspace; WORKSPACE_COUNT],
    current_ws: usize,
    launcher: Launcher,
    menu: SystemMenu,
    overlay: Overlay,
    wifi: WifiUi,
    pub status: StatusBar,
    pub exclusive: ExclusiveManager,
    pub dirty: bool,
    theme: Theme,
    content_gen: u16,
    anim: u16,
    idle_ms: u32,
    standby: bool,
    /// After a wake press, swallow Click/LongPress/Release from the same hold.
    swallow_wake: bool,
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

impl Shell {
    pub fn new() -> Self {
        Self {
            decoder: ButtonDecoder::new(),
            registry: AppRegistry::new(),
            workspaces: [Workspace::new(), Workspace::new()],
            current_ws: 0,
            launcher: Launcher::new(),
            menu: SystemMenu::new(),
            overlay: Overlay::None,
            wifi: WifiUi::new(),
            status: StatusBar::new(),
            exclusive: ExclusiveManager::new(),
            dirty: true,
            theme: Theme::Dark,
            content_gen: 0,
            anim: 0,
            idle_ms: 0,
            standby: false,
            swallow_wake: false,
        }
    }

    /// Scene change inside the same overlay (game start / death). Never a live tick.
    pub fn request_full(&mut self) {
        self.content_gen = self.content_gen.wrapping_add(1);
        self.dirty = true;
    }

    /// Sprite-only animation tick. Compositor must not wipe the playfield.
    pub fn request_live(&mut self) {
        self.anim = self.anim.wrapping_add(1);
        self.dirty = true;
    }

    pub fn content_gen(&self) -> u16 {
        self.content_gen
    }

    pub fn anim(&self) -> u16 {
        self.anim
    }

    /// Advance the status-bar clock. Returns true when `HH:MM` changed.
    pub fn advance_clock(&mut self, secs: u32) -> bool {
        if self.status.clock.add_secs(secs) {
            self.dirty = true;
            true
        } else {
            false
        }
    }

    pub fn theme(&self) -> Theme {
        self.theme
    }

    pub fn overlay(&self) -> Overlay {
        self.overlay
    }

    /// Backlight-off idle standby (PWM 0). Overlay and focused app stay put.
    pub fn is_standby(&self) -> bool {
        self.standby
    }

    /// Plug / unplug. Charging cancels idle and restores the previous brightness
    /// if the panel was already blanked.
    pub fn set_charging(&mut self, on: bool) -> EventOutcome {
        if self.status.charging != on {
            self.status.charging = on;
            self.dirty = true;
        }
        if on {
            self.idle_ms = 0;
            if self.standby {
                return self.leave_standby();
            }
        }
        EventOutcome::empty()
    }

    pub fn launcher(&self) -> &Launcher {
        &self.launcher
    }

    pub fn menu_selected(&self) -> usize {
        self.menu.selected
    }

    pub fn wifi(&self) -> &WifiUi {
        &self.wifi
    }

    pub fn wifi_connect_ssid(&self) -> &str {
        self.wifi.pending_ssid()
    }

    pub fn wifi_connect_pass(&self) -> &str {
        self.wifi.pending_pass()
    }

    pub fn wifi_connect_open(&self) -> bool {
        self.wifi.pending_open()
    }

    /// Firmware injects scan results. Host tests drive the same entry.
    pub fn apply_wifi_scan(&mut self, nets: &[WifiNet]) {
        self.wifi.apply_scan(nets);
        self.dirty = true;
    }

    pub fn apply_wifi_result(&mut self, ok: bool) {
        self.wifi.apply_result(ok);
        self.dirty = true;
        if ok {
            self.status.radio = RadioMode::Wifi;
        }
    }

    pub fn current_workspace(&self) -> usize {
        self.current_ws
    }

    pub fn tile_count(&self) -> usize {
        self.workspaces[self.current_ws].tile_count()
    }

    pub fn layout(&self) -> TileLayout {
        layout_tiles(self.tile_count())
    }

    pub fn focused_app_name(&self) -> Option<&'static str> {
        let id = self.workspaces[self.current_ws].focused_id()?;
        self.registry.get(id).map(|s| s.name)
    }

    pub fn decoder(&self) -> &ButtonDecoder {
        &self.decoder
    }

    pub fn register_app(&mut self, id: AppId, name: &'static str) -> Result<(), ()> {
        self.registry.register(id, name)
    }

    /// Feed one millivolt sample — the same entry point used by ADC and console inject.
    pub fn tick_mv(&mut self, mv: u16, dt_ms: u32) -> EventOutcome {
        let mut out = EventOutcome::empty();
        if self.status.charging {
            self.idle_ms = 0;
            if self.standby {
                merge_side(&mut out.side, self.leave_standby().side);
            }
        }
        let events = self.decoder.feed(mv, dt_ms);
        for ev in events {
            let ev_out = self.handle_event(ev);
            append(&mut out.lifecycle, ev_out.lifecycle);
            merge_side(&mut out.side, ev_out.side);
        }
        // Release is emitted before Click. Clear only after this sample is
        // Released so the wake Click cannot steal the launcher / game.
        if self.swallow_wake && matches!(decode_millivolts(mv), KeyState::Released) {
            self.swallow_wake = false;
        }
        let held = !matches!(decode_millivolts(mv), KeyState::Released);
        if self.status.charging || held || self.swallow_wake {
            self.idle_ms = 0;
        } else {
            self.idle_ms = self.idle_ms.saturating_add(dt_ms);
            if !self.standby && self.idle_ms >= IDLE_STANDBY_MS {
                merge_side(&mut out.side, self.enter_standby().side);
            }
        }
        self.refresh_status();
        out
    }

    /// Synthesize a click via millivolt windows (not a bypass).
    pub fn synth_click(&mut self, key: Key) -> EventOutcome {
        let mut out = EventOutcome::empty();
        let mv = key.typical_mv();
        for _ in 0..5 {
            let t = self.tick_mv(mv, TICK_MS);
            append(&mut out.lifecycle, t.lifecycle);
            merge_side(&mut out.side, t.side);
        }
        for _ in 0..5 {
            let t = self.tick_mv(TYPICAL_RELEASED_MV, TICK_MS);
            append(&mut out.lifecycle, t.lifecycle);
            merge_side(&mut out.side, t.side);
        }
        out
    }

    /// Synthesize a long-press via millivolt windows (not a bypass).
    pub fn synth_long(&mut self, key: Key) -> EventOutcome {
        let mut out = EventOutcome::empty();
        let mv = key.typical_mv();
        let ticks = (LONG_PRESS_MS / TICK_MS) + 4;
        for _ in 0..ticks {
            let t = self.tick_mv(mv, TICK_MS);
            append(&mut out.lifecycle, t.lifecycle);
            merge_side(&mut out.side, t.side);
        }
        for _ in 0..5 {
            let t = self.tick_mv(TYPICAL_RELEASED_MV, TICK_MS);
            append(&mut out.lifecycle, t.lifecycle);
            merge_side(&mut out.side, t.side);
        }
        out
    }

    pub fn apply_command(&mut self, cmd: Command) -> CommandOutcome {
        let mut out = CommandOutcome {
            lifecycle: Vec::new(),
            reply: String::new(),
            side: SideEffect::None,
        };
        match cmd {
            Command::Help => {
                let _ = out.reply.push_str(HELP);
            }
            Command::Status => {
                self.refresh_status();
                let _ = out.reply.push_str(self.status.format().as_str());
            }
            Command::Apps => {
                let _ = write!(out.reply, "apps:");
                for slot in self.registry.slots() {
                    let _ = write!(
                        out.reply,
                        " {}{}{}",
                        slot.name,
                        if slot.running { "*" } else { "" },
                        if slot.focused { "+" } else { "" }
                    );
                }
            }
            Command::Launcher => {
                self.open_launcher();
                self.describe_launcher(&mut out.reply);
            }
            Command::Menu => {
                self.open_menu();
                let _ = out.reply.push_str("menu");
            }
            Command::Keys => {
                self.overlay = Overlay::Keys;
                self.dirty = true;
                let _ = out.reply.push_str("keys");
            }
            Command::About => {
                self.overlay = Overlay::About;
                self.dirty = true;
                let _ = out.reply.push_str("about");
            }
            Command::Theme(t) => {
                self.theme = t;
                self.dirty = true;
                let _ = write!(out.reply, "theme={}", t.as_str());
            }
            Command::ThemeToggle => {
                self.theme = self.theme.toggle();
                self.dirty = true;
                let _ = write!(out.reply, "theme={}", self.theme.as_str());
            }
            Command::Time(None) => {
                let _ = write!(out.reply, "time {}", self.status.clock.format_hm());
            }
            Command::Time(Some((h, m))) => match self.status.clock.set_hm(h, m) {
                Ok(()) => {
                    self.dirty = true;
                    out.side = SideEffect::ClockSet;
                    let _ = write!(out.reply, "time {}", self.status.clock.format_hm());
                }
                Err(()) => {
                    let _ = out.reply.push_str("time bad");
                }
            },
            Command::Workspace(n) => {
                append(&mut out.lifecycle, self.switch_workspace(n as usize));
                let _ = write!(out.reply, "workspace {}", n);
            }
            Command::Filter(p) => {
                if !self.launcher.open {
                    self.open_launcher();
                }
                self.launcher.set_filter(p.as_str());
                self.describe_launcher(&mut out.reply);
            }
            Command::ActivateSelected => {
                append(&mut out.lifecycle, self.activate_selected());
                self.refresh_status();
                let _ = out.reply.push_str(self.status.format().as_str());
            }
            Command::ActivateName(name) => {
                if !self.launcher.open {
                    self.open_launcher();
                }
                self.launcher.clear_filter();
                let items = self.launcher.items(self.registry.slots());
                if let Some(idx) = items.iter().position(|i| i.name.eq_ignore_ascii_case(name.as_str()))
                {
                    self.launcher.selected = idx;
                    append(&mut out.lifecycle, self.activate_selected());
                } else {
                    let _ = write!(out.reply, "no app {name}");
                }
                if out.reply.is_empty() {
                    self.refresh_status();
                    let _ = out.reply.push_str(self.status.format().as_str());
                }
            }
            Command::KeyMv { mv, ticks } => {
                for _ in 0..ticks {
                    let t = self.tick_mv(mv, TICK_MS);
                    append(&mut out.lifecycle, t.lifecycle);
                    merge_side(&mut out.side, t.side);
                }
                let _ = write!(out.reply, "mv={mv} state={:?}", self.decoder.current());
            }
            Command::KeyClick(k) => {
                let t = self.synth_click(k);
                append(&mut out.lifecycle, t.lifecycle);
                merge_side(&mut out.side, t.side);
                self.refresh_status();
                let _ = out.reply.push_str(self.status.format().as_str());
            }
            Command::KeyLong(k) => {
                let t = self.synth_long(k);
                append(&mut out.lifecycle, t.lifecycle);
                merge_side(&mut out.side, t.side);
                self.refresh_status();
                let _ = out.reply.push_str(self.status.format().as_str());
            }
            Command::Brightness(n) => {
                self.status.brightness = n;
                out.side = SideEffect::SetBrightness(n);
                let _ = write!(out.reply, "brightness {n}");
            }
            Command::RadioWifi => {
                out.side = self.open_wifi();
                let _ = out.reply.push_str("radio=wifi");
            }
            Command::RadioBle => {
                let _ = self.exclusive.acquire(crate::radio::Resource::Ble);
                self.status.radio = RadioMode::Ble;
                out.side = SideEffect::BleAdvertise;
                let _ = out.reply.push_str("radio=ble");
            }
            Command::RadioOff => {
                self.exclusive.release(crate::radio::Resource::Wifi);
                self.exclusive.release(crate::radio::Resource::Ble);
                self.status.radio = RadioMode::Off;
                out.side = SideEffect::RadioOff;
                let _ = out.reply.push_str("radio=off");
            }
            Command::SleepLight => {
                out.side = SideEffect::SleepLight;
                let _ = out.reply.push_str("sleep light");
            }
            Command::SleepDeep => {
                out.side = SideEffect::SleepDeep;
                let _ = out.reply.push_str("sleep deep");
            }
            Command::Nfc => {
                let _ = out.reply.push_str(crate::nfc::describe().as_str());
            }
            Command::Probe => {
                out.side = SideEffect::Probe;
                let _ = out.reply.push_str("probe");
            }
            Command::AudioBeep => {
                let _ = self.exclusive.acquire(crate::radio::Resource::Audio);
                out.side = SideEffect::AudioBeep;
                let _ = out.reply.push_str("audio beep");
            }
            Command::AudioRec => {
                let _ = self.exclusive.acquire(crate::radio::Resource::Audio);
                out.side = SideEffect::AudioRec;
                let _ = out.reply.push_str("audio rec");
            }
        }
        self.dirty = true;
        self.refresh_status();
        out
    }

    pub fn handle_event(&mut self, ev: ButtonEvent) -> EventOutcome {
        if self.standby {
            let out = self.leave_standby();
            if !matches!(ev, ButtonEvent::Release(_)) {
                self.swallow_wake = true;
            }
            return out;
        }
        if self.swallow_wake {
            return EventOutcome::empty();
        }
        // Press/Release must not force a full LCD paint: SPI fill is tens of ms
        // and would skip ADC samples while a ladder press is still settling.
        if !matches!(ev, ButtonEvent::Press(_) | ButtonEvent::Release(_)) {
            self.dirty = true;
        }
        match self.overlay {
            Overlay::Launcher => self.handle_launcher(ev),
            Overlay::System => self.handle_menu(ev),
            Overlay::Keys | Overlay::About => self.handle_page(ev),
            Overlay::Wifi => self.handle_wifi(ev),
            Overlay::None => self.handle_workspace(ev),
        }
    }

    fn playfield_owns_dpad(&self) -> bool {
        matches!(
            self.workspaces[self.current_ws].focused_id(),
            Some(id)
                if id == crate::flap::FLAP_APP_ID
                    || id == crate::stack::STACK_APP_ID
                    || id == crate::brick::BRICK_APP_ID
        )
    }

    fn handle_workspace(&mut self, ev: ButtonEvent) -> EventOutcome {
        match ev {
            ButtonEvent::LongPress(Key::Ok) => {
                self.open_launcher();
                EventOutcome::empty()
            }
            // Games own UP/DOWN: Brick hold-to-move, Flap press, etc.
            // Release emits Click; that must not steal focus to a neighbour
            // tile. An 800 ms paddle hold must not switch workspaces.
            _ if self.playfield_owns_dpad() => {
                let mut notes = Vec::new();
                if let Some(id) = self.workspaces[self.current_ws].focused_id() {
                    let _ = notes.push(AppLifecycle::Input(id, ev));
                }
                EventOutcome::from_lifecycle(notes)
            }
            ButtonEvent::LongPress(Key::Up) => EventOutcome::from_lifecycle(self.switch_workspace(0)),
            ButtonEvent::LongPress(Key::Down) => EventOutcome::from_lifecycle(self.switch_workspace(1)),
            ButtonEvent::Click(Key::Up) => EventOutcome::from_lifecycle(self.focus_delta(-1)),
            ButtonEvent::Click(Key::Down) => EventOutcome::from_lifecycle(self.focus_delta(1)),
            ButtonEvent::Click(Key::Ok) => {
                let mut notes = Vec::new();
                if let Some(id) = self.workspaces[self.current_ws].focused_id() {
                    let _ = notes.push(AppLifecycle::Input(id, ev));
                } else {
                    self.open_launcher();
                }
                EventOutcome::from_lifecycle(notes)
            }
            ButtonEvent::Press(_) | ButtonEvent::Release(_) => {
                let mut notes = Vec::new();
                if let Some(id) = self.workspaces[self.current_ws].focused_id() {
                    let _ = notes.push(AppLifecycle::Input(id, ev));
                }
                EventOutcome::from_lifecycle(notes)
            }
        }
    }

    fn handle_launcher(&mut self, ev: ButtonEvent) -> EventOutcome {
        let len = self.launcher.items(self.registry.slots()).len();
        match ev {
            ButtonEvent::LongPress(Key::Ok) => {
                self.launcher.close();
                self.overlay = Overlay::None;
                EventOutcome::empty()
            }
            ButtonEvent::Click(Key::Up) => {
                self.launcher.move_sel(-1, len);
                EventOutcome::empty()
            }
            ButtonEvent::Click(Key::Down) => {
                self.launcher.move_sel(1, len);
                EventOutcome::empty()
            }
            ButtonEvent::Click(Key::Ok) => EventOutcome::from_lifecycle(self.activate_selected()),
            _ => EventOutcome::empty(),
        }
    }

    fn handle_menu(&mut self, ev: ButtonEvent) -> EventOutcome {
        match ev {
            ButtonEvent::Click(Key::Up) => {
                self.menu.move_sel(-1);
                EventOutcome::empty()
            }
            ButtonEvent::Click(Key::Down) => {
                self.menu.move_sel(1);
                EventOutcome::empty()
            }
            ButtonEvent::Click(Key::Ok) | ButtonEvent::LongPress(Key::Ok) => {
                let action = self.menu.current().action;
                EventOutcome {
                    lifecycle: Vec::new(),
                    side: self.apply_menu_action(action),
                }
            }
            _ => EventOutcome::empty(),
        }
    }

    /// Same [`SideEffect`] values as the matching USB console commands.
    fn apply_menu_action(&mut self, action: MenuAction) -> SideEffect {
        match action {
            MenuAction::BrightnessDec => {
                self.status.brightness = self.status.brightness.saturating_sub(10);
                SideEffect::SetBrightness(self.status.brightness)
            }
            MenuAction::BrightnessInc => {
                self.status.brightness = (self.status.brightness + 10).min(100);
                SideEffect::SetBrightness(self.status.brightness)
            }
            MenuAction::RadioOff => {
                self.status.radio = RadioMode::Off;
                self.exclusive.release(crate::radio::Resource::Wifi);
                self.exclusive.release(crate::radio::Resource::Ble);
                SideEffect::RadioOff
            }
            MenuAction::RadioWifi => self.open_wifi(),
            MenuAction::RadioBle => {
                let _ = self.exclusive.acquire(crate::radio::Resource::Ble);
                self.status.radio = RadioMode::Ble;
                SideEffect::BleAdvertise
            }
            MenuAction::SleepLight => {
                self.menu.close();
                self.overlay = Overlay::None;
                SideEffect::SleepLight
            }
            MenuAction::SleepDeep => {
                self.menu.close();
                self.overlay = Overlay::None;
                SideEffect::SleepDeep
            }
            MenuAction::Keys => {
                self.overlay = Overlay::Keys;
                SideEffect::None
            }
            MenuAction::About => {
                self.overlay = Overlay::About;
                SideEffect::None
            }
            MenuAction::ThemeToggle => {
                self.theme = self.theme.toggle();
                SideEffect::None
            }
            MenuAction::Close => {
                self.open_launcher();
                SideEffect::None
            }
        }
    }

    fn open_wifi(&mut self) -> SideEffect {
        let _ = self.exclusive.acquire(crate::radio::Resource::Wifi);
        self.status.radio = RadioMode::Wifi;
        self.wifi.begin_scan();
        self.overlay = Overlay::Wifi;
        self.dirty = true;
        SideEffect::WifiScan
    }

    fn handle_wifi(&mut self, ev: ButtonEvent) -> EventOutcome {
        match ev {
            ButtonEvent::LongPress(Key::Ok) => {
                self.open_launcher();
                EventOutcome::empty()
            }
            other => EventOutcome {
                lifecycle: Vec::new(),
                side: match self.wifi.handle(other) {
                    WifiAction::None => SideEffect::None,
                    WifiAction::Scan => SideEffect::WifiScan,
                    WifiAction::Connect => SideEffect::WifiConnect,
                },
            },
        }
    }

    /// Keys / About pages live under the system menu. Click returns there.
    fn handle_page(&mut self, ev: ButtonEvent) -> EventOutcome {
        match ev {
            ButtonEvent::LongPress(Key::Ok) => {
                self.open_launcher();
                EventOutcome::empty()
            }
            ButtonEvent::Click(_) => {
                self.overlay = Overlay::System;
                if !self.menu.open {
                    self.menu.open();
                }
                EventOutcome::empty()
            }
            _ => EventOutcome::empty(),
        }
    }

    fn enter_standby(&mut self) -> EventOutcome {
        if self.standby {
            return EventOutcome::empty();
        }
        self.standby = true;
        EventOutcome {
            lifecycle: Vec::new(),
            side: SideEffect::SetBrightness(0),
        }
    }

    fn leave_standby(&mut self) -> EventOutcome {
        if !self.standby {
            return EventOutcome::empty();
        }
        self.standby = false;
        self.idle_ms = 0;
        EventOutcome {
            lifecycle: Vec::new(),
            side: SideEffect::SetBrightness(self.status.brightness),
        }
    }

    /// Home screen: unified launcher. Call at boot so the panel is immediately usable.
    pub fn enter_home(&mut self) {
        self.open_launcher();
        self.refresh_status();
    }

    fn open_launcher(&mut self) {
        self.menu.close();
        self.launcher.open();
        self.overlay = Overlay::Launcher;
        self.dirty = true;
    }

    fn open_menu(&mut self) {
        self.launcher.close();
        self.menu.open();
        self.overlay = Overlay::System;
        self.dirty = true;
    }

    fn activate_selected(&mut self) -> Vec<AppLifecycle, 8> {
        let item = match self.launcher.selected_item(self.registry.slots()) {
            Some(i) => i,
            None => return Vec::new(),
        };
        self.launcher.close();
        match item.kind {
            LauncherKind::System => {
                self.open_menu();
                Vec::new()
            }
            LauncherKind::App(id) => {
                self.overlay = Overlay::None;
                self.start_on_current(id)
            }
        }
    }

    pub fn start_on_current(&mut self, id: AppId) -> Vec<AppLifecycle, 8> {
        let mut notes = Vec::new();
        append(&mut notes, self.registry.start(id));
        let ws = &mut self.workspaces[self.current_ws];
        if let Some(idx) = ws.slots.iter().position(|s| *s == id) {
            ws.focused = idx;
        } else if ws.slots.len() < TILES_PER_WORKSPACE {
            let _ = ws.slots.push(id);
            ws.focused = ws.slots.len() - 1;
        } else {
            // Replace the focused tile.
            let old = ws.slots[ws.focused];
            ws.slots[ws.focused] = id;
            if !self.app_on_any_workspace(old) {
                append(&mut notes, self.registry.stop(old));
            }
        }
        append(&mut notes, self.registry.focus(id));
        self.dirty = true;
        notes
    }

    fn app_on_any_workspace(&self, id: AppId) -> bool {
        self.workspaces
            .iter()
            .any(|w| w.slots.iter().any(|s| *s == id))
    }

    fn switch_workspace(&mut self, idx: usize) -> Vec<AppLifecycle, 8> {
        let idx = idx.min(WORKSPACE_COUNT - 1);
        let mut notes = Vec::new();
        if idx == self.current_ws {
            return notes;
        }
        if let Some(id) = self.workspaces[self.current_ws].focused_id() {
            if let Some(slot) = self.registry.get_mut(id) {
                if slot.focused {
                    slot.focused = false;
                    let _ = notes.push(AppLifecycle::Blur(id));
                }
            }
        }
        self.current_ws = idx;
        if let Some(id) = self.workspaces[self.current_ws].focused_id() {
            append(&mut notes, self.registry.focus(id));
        }
        self.overlay = Overlay::None;
        self.launcher.close();
        self.dirty = true;
        notes
    }

    fn focus_delta(&mut self, delta: i16) -> Vec<AppLifecycle, 8> {
        let mut notes = Vec::new();
        let ws = &mut self.workspaces[self.current_ws];
        let n = ws.slots.len();
        if n <= 1 {
            if let Some(id) = ws.focused_id() {
                let _ = notes.push(AppLifecycle::Input(
                    id,
                    if delta < 0 {
                        ButtonEvent::Click(Key::Up)
                    } else {
                        ButtonEvent::Click(Key::Down)
                    },
                ));
            }
            return notes;
        }
        let mut s = ws.focused as i16 + delta;
        let ni = n as i16;
        s = ((s % ni) + ni) % ni;
        ws.focused = s as usize;
        if let Some(id) = ws.focused_id() {
            append(&mut notes, self.registry.focus(id));
        }
        notes
    }

    fn describe_launcher(&self, reply: &mut String<256>) {
        let items = self.launcher.visible_names(self.registry.slots());
        let _ = write!(
            reply,
            "launcher open={} sel={} filter='{}' items=",
            self.launcher.open,
            self.launcher.selected,
            self.launcher.filter()
        );
        for (i, n) in items.iter().enumerate() {
            if i > 0 {
                let _ = reply.push(',');
            }
            let _ = reply.push_str(n);
        }
    }

    pub fn refresh_status(&mut self) {
        self.status.workspace = self.current_ws as u8;
        match self.overlay {
            Overlay::Launcher => {
                self.status.overlay.clear();
                let _ = self.status.overlay.push_str("launcher");
                self.status.set_focused("launcher");
            }
            Overlay::System => {
                self.status.overlay.clear();
                let _ = self.status.overlay.push_str("menu");
                self.status.set_focused("system");
            }
            Overlay::Keys => {
                self.status.overlay.clear();
                let _ = self.status.overlay.push_str("keys");
                self.status.set_focused("keys");
            }
            Overlay::About => {
                self.status.overlay.clear();
                let _ = self.status.overlay.push_str("about");
                self.status.set_focused("about");
            }
            Overlay::Wifi => {
                self.status.overlay.clear();
                let _ = self.status.overlay.push_str("wifi");
                self.status.set_focused("wifi");
            }
            Overlay::None => {
                self.status.overlay.clear();
                if let Some(name) = self.focused_app_name() {
                    self.status.set_focused(name);
                } else {
                    self.status.set_focused("shell");
                }
            }
        }
    }

    pub fn launcher_names(&self) -> heapless::Vec<&'static str, 12> {
        self.launcher.visible_names(self.registry.slots())
    }
}

fn append<const N: usize, const M: usize>(dst: &mut Vec<AppLifecycle, N>, src: Vec<AppLifecycle, M>) {
    for n in src {
        let _ = dst.push(n);
    }
}
