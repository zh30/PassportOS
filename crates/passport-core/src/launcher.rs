//! Unified launcher: open, filter by prefix, activate.

use heapless::{String, Vec};

use crate::app::{AppId, AppSlot};

pub const MAX_LAUNCHER_ITEMS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LauncherKind {
    App(AppId),
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LauncherItem {
    pub kind: LauncherKind,
    pub name: &'static str,
}

#[derive(Clone, Debug)]
pub struct Launcher {
    pub open: bool,
    pub selected: usize,
    filter: String<16>,
}

impl Default for Launcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Launcher {
    pub const fn new() -> Self {
        Self {
            open: false,
            selected: 0,
            filter: String::new(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn filter(&self) -> &str {
        self.filter.as_str()
    }

    pub fn set_filter(&mut self, prefix: &str) {
        self.filter.clear();
        let _ = self.filter.push_str(prefix);
        self.selected = 0;
    }

    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.selected = 0;
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.selected = 0;
        }
    }

    pub fn open(&mut self) {
        self.open = true;
        self.selected = 0;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn items(&self, apps: &[AppSlot]) -> Vec<LauncherItem, MAX_LAUNCHER_ITEMS> {
        let mut items = Vec::new();
        let f = self.filter.as_str();
        for slot in apps {
            if matches_filter(slot.name, f) {
                let _ = items.push(LauncherItem {
                    kind: LauncherKind::App(slot.id),
                    name: slot.name,
                });
            }
        }
        if matches_filter("system", f) {
            let _ = items.push(LauncherItem {
                kind: LauncherKind::System,
                name: "system",
            });
        }
        items
    }

    pub fn visible_names(&self, apps: &[AppSlot]) -> Vec<&'static str, MAX_LAUNCHER_ITEMS> {
        let mut names = Vec::new();
        for item in self.items(apps) {
            let _ = names.push(item.name);
        }
        names
    }

    pub fn move_sel(&mut self, delta: i16, len: usize) {
        if len == 0 {
            self.selected = 0;
            return;
        }
        let n = len as i16;
        let mut s = self.selected as i16 + delta;
        s = ((s % n) + n) % n;
        self.selected = s as usize;
    }

    pub fn selected_item(&self, apps: &[AppSlot]) -> Option<LauncherItem> {
        let items = self.items(apps);
        items.get(self.selected).copied()
    }
}

fn matches_filter(name: &str, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    name.len() >= filter.len()
        && name
            .as_bytes()
            .iter()
            .zip(filter.as_bytes())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}
