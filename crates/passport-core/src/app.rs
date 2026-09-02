//! In-firmware app contract: static inventory, lifecycle, start/stop/focus.

use heapless::Vec;

pub const MAX_APPS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AppId(pub u8);

impl AppId {
    pub const SHELL: AppId = AppId(0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppSlot {
    pub id: AppId,
    pub name: &'static str,
    pub running: bool,
    pub focused: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppLifecycle {
    Start(AppId),
    Stop(AppId),
    Focus(AppId),
    Blur(AppId),
    Input(AppId, crate::input::ButtonEvent),
}

#[derive(Clone, Debug)]
pub struct AppRegistry {
    slots: Vec<AppSlot, MAX_APPS>,
}

impl Default for AppRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AppRegistry {
    pub const fn new() -> Self {
        Self { slots: Vec::new() }
    }

    pub fn register(&mut self, id: AppId, name: &'static str) -> Result<(), ()> {
        if self.slots.iter().any(|s| s.id == id || s.name == name) {
            return Err(());
        }
        self.slots
            .push(AppSlot {
                id,
                name,
                running: false,
                focused: false,
            })
            .map_err(|_| ())
    }

    pub fn get(&self, id: AppId) -> Option<&AppSlot> {
        self.slots.iter().find(|s| s.id == id)
    }

    pub fn get_mut(&mut self, id: AppId) -> Option<&mut AppSlot> {
        self.slots.iter_mut().find(|s| s.id == id)
    }

    pub fn by_name(&self, name: &str) -> Option<&AppSlot> {
        self.slots.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    pub fn slots(&self) -> &[AppSlot] {
        &self.slots
    }

    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.slots.iter().map(|s| s.name)
    }

    /// Mark running and return lifecycle notes. Idempotent if already running.
    pub fn start(&mut self, id: AppId) -> Vec<AppLifecycle, 4> {
        let mut notes = Vec::new();
        if let Some(slot) = self.get_mut(id) {
            if !slot.running {
                slot.running = true;
                let _ = notes.push(AppLifecycle::Start(id));
            }
        }
        notes
    }

    pub fn stop(&mut self, id: AppId) -> Vec<AppLifecycle, 4> {
        let mut notes = Vec::new();
        if let Some(slot) = self.get_mut(id) {
            if slot.running {
                if slot.focused {
                    slot.focused = false;
                    let _ = notes.push(AppLifecycle::Blur(id));
                }
                slot.running = false;
                let _ = notes.push(AppLifecycle::Stop(id));
            }
        }
        notes
    }

    pub fn focus(&mut self, id: AppId) -> Vec<AppLifecycle, 4> {
        let mut notes = Vec::new();
        let mut prev: Option<AppId> = None;
        for slot in self.slots.iter_mut() {
            if slot.focused && slot.id != id {
                slot.focused = false;
                prev = Some(slot.id);
            }
        }
        if let Some(p) = prev {
            let _ = notes.push(AppLifecycle::Blur(p));
        }
        if let Some(slot) = self.get_mut(id) {
            if slot.running && !slot.focused {
                slot.focused = true;
                let _ = notes.push(AppLifecycle::Focus(id));
            }
        }
        notes
    }

    pub fn focused(&self) -> Option<&AppSlot> {
        self.slots.iter().find(|s| s.focused)
    }
}
