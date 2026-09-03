//! Unified launcher: three islands on the first screen, then a group.
//!
//! Home is not an 8-row dump. UP/DOWN on islands; OK drills in. UP on the
//! first app of a group zooms back out. Filter (console) is a flat list.

use heapless::{String, Vec};

use crate::app::{AppId, AppSlot};
use crate::board::LCD_H;

pub const MAX_LAUNCHER_ITEMS: usize = 12;
/// Group top, below the status bar. Must match firmware chrome.
pub const LAUNCHER_Y: u16 = 40;
pub const LAUNCHER_ROW_H: u16 = 44;
/// Three islands fill the panel: 40 + 3×88 = 304 < 320.
pub const ISLAND_H: u16 = 88;
pub const ISLAND_COUNT: usize = 3;
/// Inset group width. Firmware chrome.
pub const LAUNCHER_CARD_W: u16 = 208;
pub const ISLAND_PAD: u16 = 12;
/// 22px mark — leaves 23 caption columns after pad + chevron.
pub const ISLAND_ICON: u16 = 22;
pub const ISLAND_GAP: u16 = 8;
pub const FONT_CELL_W: u16 = 6;
pub const FONT_2X_W: u16 = 12;
const ISLAND_CHEVRON_W: u16 = 6;
const ISLAND_ICON_GAP: u16 = 10;
/// 2x title (16) + 4px gap + 1x caption (8).
pub const ISLAND_TEXT_STACK: u16 = 28;
const ISLAND_TEXT_CHEVRON_GAP: u16 = 4;

/// How many rows fit under `LAUNCHER_Y` without painting past the panel.
pub const fn launcher_visible() -> usize {
    ((LCD_H - LAUNCHER_Y) / LAUNCHER_ROW_H) as usize
}

/// Top of island `i`. Must not use [`LAUNCHER_ROW_H`] — that is the group list.
pub const fn island_card_y(i: usize) -> u16 {
    LAUNCHER_Y.saturating_add((i as u16).saturating_mul(ISLAND_H))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LauncherGroup {
    Play,
    Tools,
}

impl LauncherGroup {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Tools => "tools",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Play => "Play",
            Self::Tools => "Tools",
        }
    }

    pub const fn island_index(self) -> usize {
        match self {
            Self::Play => 0,
            Self::Tools => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LauncherView {
    Islands,
    Group(LauncherGroup),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LauncherKind {
    App(AppId),
    System,
    Island(LauncherGroup),
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
    view: LauncherView,
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
            view: LauncherView::Islands,
            filter: String::new(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn view(&self) -> LauncherView {
        if self.filter.is_empty() {
            self.view
        } else {
            LauncherView::Islands
        }
    }

    /// 0 = islands, 1 = play, 2 = tools, 3 = filter list.
    pub fn view_id(&self) -> u8 {
        if !self.filter.is_empty() {
            3
        } else {
            match self.view {
                LauncherView::Islands => 0,
                LauncherView::Group(LauncherGroup::Play) => 1,
                LauncherView::Group(LauncherGroup::Tools) => 2,
            }
        }
    }

    pub fn is_islands(&self) -> bool {
        self.filter.is_empty() && matches!(self.view, LauncherView::Islands)
    }

    pub fn filter(&self) -> &str {
        self.filter.as_str()
    }

    pub fn set_filter(&mut self, prefix: &str) {
        self.filter.clear();
        let _ = self.filter.push_str(prefix);
        self.selected = 0;
        if !self.filter.is_empty() {
            self.view = LauncherView::Islands;
        }
    }

    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.selected = 0;
        self.view = LauncherView::Islands;
    }

    pub fn enter_group(&mut self, g: LauncherGroup) {
        self.view = LauncherView::Group(g);
        self.selected = 0;
    }

    pub fn leave_group(&mut self) {
        let sel = match self.view {
            LauncherView::Group(g) => g.island_index(),
            LauncherView::Islands => self.selected,
        };
        self.view = LauncherView::Islands;
        self.selected = sel;
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.selected = 0;
            self.view = LauncherView::Islands;
        }
    }

    pub fn open(&mut self) {
        self.open = true;
        self.selected = 0;
        self.view = LauncherView::Islands;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.view = LauncherView::Islands;
        self.selected = 0;
    }

    /// Catalog: every app plus System. Used by `activate <name>`.
    pub fn catalog(&self, apps: &[AppSlot]) -> Vec<LauncherItem, MAX_LAUNCHER_ITEMS> {
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

    /// What UP/DOWN/paint see. Islands on home; a group after OK; flat when filtered.
    pub fn items(&self, apps: &[AppSlot]) -> Vec<LauncherItem, MAX_LAUNCHER_ITEMS> {
        if !self.filter.is_empty() {
            return self.catalog(apps);
        }
        match self.view {
            LauncherView::Islands => islands(apps),
            LauncherView::Group(g) => group_items(g, apps),
        }
    }

    pub fn visible_names(&self, apps: &[AppSlot]) -> Vec<&'static str, MAX_LAUNCHER_ITEMS> {
        let mut names = Vec::new();
        for item in self.items(apps) {
            let _ = names.push(item.name);
        }
        names
    }

    pub fn island_members<'a>(
        g: LauncherGroup,
        apps: &'a [AppSlot],
    ) -> Vec<&'static str, MAX_LAUNCHER_ITEMS> {
        let mut names = Vec::new();
        for slot in apps {
            if classify(slot.name) == g {
                let _ = names.push(slot.name);
            }
        }
        names
    }

    /// Visible slice `[start, start+len)`. Islands always fit; groups usually do.
    pub fn window(&self, n: usize, max: usize) -> (usize, usize) {
        let max = if self.is_islands() {
            ISLAND_COUNT.max(1)
        } else {
            max.max(1)
        };
        if n == 0 {
            return (0, 0);
        }
        if n <= max {
            return (0, n);
        }
        let sel = self.selected.min(n - 1);
        let mut start = 0usize;
        if sel >= max {
            start = sel + 1 - max;
        }
        if start + max > n {
            start = n - max;
        }
        (start, max)
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

fn islands(apps: &[AppSlot]) -> Vec<LauncherItem, MAX_LAUNCHER_ITEMS> {
    let mut items = Vec::new();
    if !group_items(LauncherGroup::Play, apps).is_empty() {
        let _ = items.push(LauncherItem {
            kind: LauncherKind::Island(LauncherGroup::Play),
            name: "play",
        });
    }
    if !group_items(LauncherGroup::Tools, apps).is_empty() {
        let _ = items.push(LauncherItem {
            kind: LauncherKind::Island(LauncherGroup::Tools),
            name: "tools",
        });
    }
    let _ = items.push(LauncherItem {
        kind: LauncherKind::System,
        name: "system",
    });
    items
}

fn group_items(g: LauncherGroup, apps: &[AppSlot]) -> Vec<LauncherItem, MAX_LAUNCHER_ITEMS> {
    let mut items = Vec::new();
    for slot in apps {
        if classify(slot.name) == g {
            let _ = items.push(LauncherItem {
                kind: LauncherKind::App(slot.id),
                name: slot.name,
            });
        }
    }
    items
}

pub fn classify(name: &str) -> LauncherGroup {
    match name {
        "flap" | "stack" | "brick" | "boo" => LauncherGroup::Play,
        _ => LauncherGroup::Tools,
    }
}

/// Short ASCII label for the 6px font. Never UTF-8 — `draw_text` walks bytes.
pub fn display_name(name: &'static str) -> &'static str {
    match name {
        "pulse" => "Pulse",
        "nfc" => "Tap",
        "flap" => "Flap",
        "stack" => "Stack",
        "brick" => "Brick",
        "boo" => "Boo",
        "tune" => "Tune",
        "play" => "Play",
        "tools" => "Tools",
        "system" => "System",
        _ => name,
    }
}

pub const fn island_card_h() -> u16 {
    ISLAND_H.saturating_sub(ISLAND_GAP)
}

/// Card-relative X of the title/caption.
pub const fn island_text_x() -> u16 {
    ISLAND_PAD + ISLAND_ICON + ISLAND_ICON_GAP
}

/// Card-relative X of the chevron glyph.
pub const fn island_chevron_x() -> u16 {
    LAUNCHER_CARD_W
        .saturating_sub(ISLAND_PAD)
        .saturating_sub(ISLAND_CHEVRON_W)
}

/// Exclusive end X for island text (gap before the chevron).
pub const fn island_text_end() -> u16 {
    island_chevron_x().saturating_sub(ISLAND_TEXT_CHEVRON_GAP)
}

pub const fn island_title_y(card_y: u16) -> u16 {
    card_y + island_card_h().saturating_sub(ISLAND_TEXT_STACK) / 2
}

pub const fn island_caption_y(card_y: u16) -> u16 {
    island_title_y(card_y).saturating_add(20)
}

pub const fn island_icon_y(card_y: u16) -> u16 {
    card_y + island_card_h().saturating_sub(ISLAND_ICON) / 2
}

pub const fn island_chevron_y(card_y: u16) -> u16 {
    island_title_y(card_y).saturating_add(4)
}

/// Caption columns inside an island (icon + pad + chevron reserved).
pub const fn island_blurb_cols() -> usize {
    (island_text_end().saturating_sub(island_text_x()) / FONT_CELL_W) as usize
}

/// Columns inside a grouped card with 12px side pads and no icon.
pub const fn grouped_inner_cols() -> usize {
    (LAUNCHER_CARD_W.saturating_sub(ISLAND_PAD * 2) / FONT_CELL_W) as usize
}

/// 5×7 glyphs are ASCII 32..126. UTF-8 walks as extra blank cells.
pub fn is_lcd_ascii(s: &str) -> bool {
    s.bytes().all(|b| (32..127).contains(&b))
}

pub fn island_caption(g: LauncherGroup, apps: &[AppSlot]) -> String<24> {
    let mut s = String::<24>::new();
    let max = island_blurb_cols();
    for slot in apps {
        if classify(slot.name) != g {
            continue;
        }
        let d = display_name(slot.name);
        if !is_lcd_ascii(d) {
            continue;
        }
        let extra = if s.is_empty() { d.len() } else { 1 + d.len() };
        if s.len() + extra > max {
            break;
        }
        if !s.is_empty() {
            let _ = s.push(' ');
        }
        let _ = s.push_str(d);
    }
    s
}

pub fn system_caption() -> &'static str {
    SYSTEM_CAPTION
}

const SYSTEM_CAPTION: &str = "wifi sleep about";
const PLAY_CAPTION_BUDGET: &str = "Flap Stack Brick Boo";
const TOOLS_CAPTION_BUDGET: &str = "Pulse Tap Tune";

const _: () = assert!(SYSTEM_CAPTION.len() <= island_blurb_cols());
const _: () = assert!(PLAY_CAPTION_BUDGET.len() <= island_blurb_cols());
const _: () = assert!(TOOLS_CAPTION_BUDGET.len() <= island_blurb_cols());
const _: () = assert!(LAUNCHER_Y + (ISLAND_COUNT as u16) * ISLAND_H <= LCD_H);
const _: () = assert!(6 * FONT_2X_W <= island_text_end().saturating_sub(island_text_x()));

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::{ABOUT, CHEATSHEET};
    use crate::menu::MENU_ITEMS;

    fn slot(id: u8, name: &'static str) -> AppSlot {
        AppSlot {
            id: AppId(id),
            name,
            running: false,
            focused: false,
            wants_mic: false,
        }
    }

    fn catalog() -> [AppSlot; 7] {
        [
            slot(1, "pulse"),
            slot(3, "nfc"),
            slot(4, "flap"),
            slot(5, "stack"),
            slot(6, "brick"),
            slot(7, "boo"),
            slot(8, "tune"),
        ]
    }

    #[test]
    fn island_captions_are_ascii_and_fit() {
        let apps = catalog();
        let play = island_caption(LauncherGroup::Play, &apps);
        assert_eq!(play.as_str(), "Flap Stack Brick Boo");
        assert!(is_lcd_ascii(play.as_str()));
        assert!(play.len() <= island_blurb_cols());

        let tools = island_caption(LauncherGroup::Tools, &apps);
        assert_eq!(tools.as_str(), "Pulse Tap Tune");
        assert!(is_lcd_ascii(tools.as_str()));
        assert!(tools.len() <= island_blurb_cols());

        assert!(is_lcd_ascii(system_caption()));
        assert!(system_caption().len() <= island_blurb_cols());
        assert!(!system_caption().as_bytes().contains(&0xC2));
    }

    #[test]
    fn two_x_island_titles_fit_the_text_column() {
        let budget = island_text_end().saturating_sub(island_text_x());
        for name in ["play", "tools", "system"] {
            let title = display_name(name);
            assert!(is_lcd_ascii(title), "{title}");
            let px = (title.len() as u16).saturating_mul(FONT_2X_W);
            assert!(px <= budget, "{title} is {px}px, text column is {budget}px");
        }
    }

    #[test]
    fn display_names_are_ascii() {
        for name in [
            "pulse", "nfc", "flap", "stack", "brick", "boo", "tune", "play", "tools", "system",
        ] {
            let d = display_name(name);
            assert!(is_lcd_ascii(d), "{d}");
            assert!(d.len() <= grouped_inner_cols(), "{d}");
        }
    }

    #[test]
    fn grouped_chrome_copy_fits_the_card() {
        let cols = grouped_inner_cols();
        for line in CHEATSHEET.iter().chain(ABOUT.iter()) {
            assert!(is_lcd_ascii(line), "{line}");
            assert!(
                line.len() <= cols,
                "{line} is {} cols, card holds {cols}",
                line.len()
            );
        }
        for item in MENU_ITEMS {
            assert!(is_lcd_ascii(item.label), "{}", item.label);
            assert!(item.label.len() <= cols, "{}", item.label);
        }
    }

    #[test]
    fn last_island_card_stays_on_the_panel() {
        let last_y = island_card_y(ISLAND_COUNT - 1);
        assert!(last_y.saturating_add(island_card_h()) <= LCD_H);
        assert!(island_blurb_cols() >= "Flap Stack Brick Boo".len());
    }

    #[test]
    fn island_cards_are_not_group_rows() {
        assert_ne!(ISLAND_H, LAUNCHER_ROW_H);
        assert_eq!(island_card_y(0), LAUNCHER_Y);
        assert_eq!(island_card_y(1), LAUNCHER_Y + ISLAND_H);
        assert_ne!(island_card_y(1), LAUNCHER_Y + LAUNCHER_ROW_H);
    }
}
