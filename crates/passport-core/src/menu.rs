//! System menu: brightness, radio, sleep, keys, about.

use heapless::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    BrightnessDec,
    BrightnessInc,
    RadioOff,
    RadioWifi,
    RadioBle,
    SleepLight,
    SleepDeep,
    ThemeToggle,
    Keys,
    About,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub label: &'static str,
    pub action: MenuAction,
}

pub const MENU_ITEMS: &[MenuItem] = &[
    MenuItem {
        label: "dimmer",
        action: MenuAction::BrightnessDec,
    },
    MenuItem {
        label: "brighter",
        action: MenuAction::BrightnessInc,
    },
    MenuItem {
        label: "appearance",
        action: MenuAction::ThemeToggle,
    },
    MenuItem {
        label: "radio off",
        action: MenuAction::RadioOff,
    },
    MenuItem {
        label: "wifi",
        action: MenuAction::RadioWifi,
    },
    MenuItem {
        label: "bluetooth",
        action: MenuAction::RadioBle,
    },
    MenuItem {
        label: "sleep",
        action: MenuAction::SleepLight,
    },
    MenuItem {
        label: "deep sleep",
        action: MenuAction::SleepDeep,
    },
    MenuItem {
        label: "keys",
        action: MenuAction::Keys,
    },
    MenuItem {
        label: "about",
        action: MenuAction::About,
    },
    MenuItem {
        label: "home",
        action: MenuAction::Close,
    },
];

#[derive(Clone, Debug)]
pub struct SystemMenu {
    pub open: bool,
    pub selected: usize,
}

impl Default for SystemMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemMenu {
    pub const fn new() -> Self {
        Self {
            open: false,
            selected: 0,
        }
    }

    pub fn open(&mut self) {
        self.open = true;
        self.selected = 0;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn move_sel(&mut self, delta: i16) {
        let n = MENU_ITEMS.len() as i16;
        let mut s = self.selected as i16 + delta;
        s = ((s % n) + n) % n;
        self.selected = s as usize;
    }

    pub fn current(&self) -> MenuItem {
        MENU_ITEMS[self.selected]
    }

    pub fn labels(&self) -> Vec<&'static str, 16> {
        let mut v = Vec::new();
        for item in MENU_ITEMS {
            let _ = v.push(item.label);
        }
        v
    }
}
