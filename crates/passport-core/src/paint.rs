//! What to redraw. Full-panel SPI fills flash on ST7789; selection changes
//! only need the two affected cards. Live animation is a separate generation
//! so a game tick never wipes the playfield.

use crate::boo::BOO_APP_ID;
use crate::brick::BRICK_APP_ID;
use crate::flap::FLAP_APP_ID;
use crate::menu::MenuAction;
use crate::shell::{Overlay, Shell};
use crate::stack::STACK_APP_ID;
use crate::status::RadioMode;
use crate::theme::Theme;
use crate::tune::TUNE_APP_ID;
use crate::wifi::WifiPhase;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameSig {
    pub overlay: Overlay,
    pub launcher_sel: usize,
    pub launcher_win: u8,
    pub launcher_view: u8,
    pub menu_sel: usize,
    pub workspace: u8,
    pub soc: Option<u8>,
    pub charging: bool,
    pub radio: RadioMode,
    pub focused: u8,
    pub theme: Theme,
    /// Scene generation inside the same overlay (game start / death / retry).
    pub content_gen: u16,
    /// Sprite-only generation. Must never imply `wipe_content`.
    pub anim: u16,
    /// Minutes since midnight when set, else `None` (`--:--`).
    pub clock_min: Option<u16>,
    pub wifi_phase: WifiPhase,
    pub wifi_sel: usize,
    /// Wi-Fi list row count (nets + footer) — footer rows appear on connect.
    pub wifi_rows_n: u8,
    pub wifi_fail: u8,
    pub vol: u8,
    pub muted: bool,
    pub ble_conn: bool,
    pub ime_idx: u8,
    pub ime_len: u8,
    pub ime_shift: bool,
    pub about_page: u8,
}

impl FrameSig {
    pub fn capture(shell: &Shell) -> Self {
        Self {
            overlay: shell.overlay(),
            launcher_sel: shell.launcher().selected,
            launcher_win: shell.launcher_window().0.min(255) as u8,
            launcher_view: shell.launcher().view_id(),
            menu_sel: shell.menu_selected(),
            workspace: shell.status.workspace,
            soc: shell.status.battery_soc,
            charging: shell.status.charging,
            radio: shell.status.radio,
            focused: shell.registry.focused().map(|s| s.id.0).unwrap_or(0),
            theme: shell.theme(),
            content_gen: shell.content_gen(),
            anim: shell.anim(),
            clock_min: shell.status.clock.minutes_of_day(),
            wifi_phase: shell.wifi().phase(),
            wifi_sel: shell.wifi().selected(),
            wifi_rows_n: shell.wifi().row_count().min(255) as u8,
            wifi_fail: shell.wifi().fail().map(|f| f as u8).unwrap_or(255),
            vol: shell.status.volume,
            muted: shell.status.muted,
            ble_conn: shell.status.ble_conn,
            ime_idx: shell.wifi().ime().cursor() as u8,
            ime_len: shell.wifi().ime().len() as u8,
            ime_shift: shell.wifi().ime().shift(),
            about_page: shell.about_page(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PaintPlan {
    /// Fill below the status bar only. Never 240×320.
    pub wipe_content: bool,
    pub status: bool,
    pub desktop: bool,
    pub control: bool,
    pub keys: bool,
    pub about: bool,
    pub pulse: bool,
    pub game: bool,
    /// Sprite patches only. Never a content wipe.
    pub game_live: bool,
    pub launcher_cards: Option<(usize, usize)>,
    pub menu_rows: Option<(usize, usize)>,
    pub pulse_meter: bool,
    pub wifi: bool,
    pub wifi_rows: Option<(usize, usize)>,
    pub ime: bool,
    pub ime_keys: Option<(usize, usize)>,
    pub ime_field: bool,
}

impl PaintPlan {
    /// Rows filled on a content wipe. 0 if none. Never the full 320-row panel.
    pub fn wipe_rows(self) -> u16 {
        if self.wipe_content {
            crate::board::LCD_H - crate::compositor::STATUS_BAR_H
        } else {
            0
        }
    }

    pub fn is_nop(self) -> bool {
        !self.wipe_content
            && !self.status
            && !self.desktop
            && !self.control
            && !self.keys
            && !self.about
            && !self.pulse
            && !self.game
            && !self.game_live
            && self.launcher_cards.is_none()
            && self.menu_rows.is_none()
            && !self.pulse_meter
            && !self.wifi
            && self.wifi_rows.is_none()
            && !self.ime
            && self.ime_keys.is_none()
            && !self.ime_field
    }

    pub fn full(now: FrameSig) -> Self {
        match now.overlay {
            Overlay::Launcher => Self {
                wipe_content: true,
                status: true,
                desktop: true,
                ..Self::default()
            },
            Overlay::System => Self {
                wipe_content: true,
                status: true,
                control: true,
                ..Self::default()
            },
            Overlay::Keys => Self {
                wipe_content: true,
                status: true,
                keys: true,
                ..Self::default()
            },
            Overlay::About => Self {
                wipe_content: true,
                status: true,
                about: true,
                ..Self::default()
            },
            Overlay::Wifi => Self {
                wipe_content: true,
                status: true,
                wifi: true,
                ..Self::default()
            },
            // Flap fills the tile itself; a compositor wipe plus a sky fill
            // would flash the ST7789 twice on every scene change.
            Overlay::None if is_live_game(now.focused) => Self {
                wipe_content: false,
                status: true,
                game: true,
                ..Self::default()
            },
            Overlay::None => Self {
                wipe_content: true,
                status: true,
                pulse: true,
                ..Self::default()
            },
        }
    }

    pub fn diff(prev: Option<FrameSig>, now: FrameSig) -> Self {
        let Some(p) = prev else {
            return Self::full(now);
        };
        if p.overlay != now.overlay
            || p.focused != now.focused
            || p.theme != now.theme
            || p.about_page != now.about_page
        {
            return Self::full(now);
        }
        let status = p.soc != now.soc
            || p.charging != now.charging
            || p.radio != now.radio
            || p.ble_conn != now.ble_conn
            || p.workspace != now.workspace
            || p.clock_min != now.clock_min;
        if now.overlay == Overlay::None && p.content_gen != now.content_gen {
            return Self {
                status,
                game: is_live_game(now.focused),
                pulse: !is_live_game(now.focused),
                ..Self::default()
            };
        }
        if now.overlay == Overlay::None && p.anim != now.anim {
            return Self {
                status,
                game_live: is_live_game(now.focused),
                pulse_meter: now.focused == 1,
                ..Self::default()
            };
        }
        match now.overlay {
            Overlay::Launcher if p.launcher_view != now.launcher_view => Self {
                status,
                wipe_content: true,
                desktop: true,
                ..Self::default()
            },
            Overlay::Launcher if p.launcher_win != now.launcher_win => Self {
                status,
                desktop: true,
                ..Self::default()
            },
            // Islands are 88px cards. The group list is 44px rows. A focus
            // move on home must not emit `launcher_cards` — firmware would
            // paint the small rows over the islands.
            Overlay::Launcher if p.launcher_sel != now.launcher_sel && now.launcher_view == 0 => {
                Self {
                    status,
                    desktop: true,
                    ..Self::default()
                }
            }
            Overlay::Launcher if p.launcher_sel != now.launcher_sel => Self {
                status,
                launcher_cards: Some((p.launcher_sel, now.launcher_sel)),
                ..Self::default()
            },
            Overlay::System if p.menu_sel != now.menu_sel => Self {
                status,
                menu_rows: Some((p.menu_sel, now.menu_sel)),
                ..Self::default()
            },
            // Volume/mute edits repaint the audio rows (accessory text).
            Overlay::System if p.vol != now.vol || p.muted != now.muted => Self {
                status,
                menu_rows: Some((
                    menu_row_of(MenuAction::VolumeInc),
                    menu_row_of(MenuAction::MuteToggle),
                )),
                ..Self::default()
            },
            Overlay::Wifi if p.wifi_phase != now.wifi_phase => Self::full(now),
            // Footer rows appear/vanish on connect, disconnect and forget.
            Overlay::Wifi if p.wifi_rows_n != now.wifi_rows_n || p.wifi_fail != now.wifi_fail => Self {
                status,
                wifi: true,
                ..Self::default()
            },
            Overlay::Wifi if now.wifi_phase == WifiPhase::List && p.wifi_sel != now.wifi_sel => {
                Self {
                    status,
                    wifi_rows: Some((p.wifi_sel, now.wifi_sel)),
                    ..Self::default()
                }
            }
            Overlay::Wifi if now.wifi_phase == WifiPhase::Ime && p.ime_shift != now.ime_shift => {
                Self {
                    status,
                    wifi: true,
                    ..Self::default()
                }
            }
            Overlay::Wifi if now.wifi_phase == WifiPhase::Ime => {
                let keys = if p.ime_idx != now.ime_idx {
                    Some((p.ime_idx as usize, now.ime_idx as usize))
                } else {
                    None
                };
                let field = p.ime_len != now.ime_len;
                Self {
                    status,
                    ime: keys.is_some() || field,
                    ime_keys: keys,
                    ime_field: field,
                    ..Self::default()
                }
            }
            Overlay::None => Self {
                status,
                pulse_meter: now.focused == 1,
                ..Self::default()
            },
            _ if status => Self {
                status: true,
                ..Self::default()
            },
            _ => Self::default(),
        }
    }
}

/// Index of a system-menu row; 0 when absent (paint then no-ops harmlessly).
fn menu_row_of(action: crate::menu::MenuAction) -> usize {
    crate::menu::MENU_ITEMS
        .iter()
        .position(|i| i.action == action)
        .unwrap_or(0)
}

fn is_live_game(id: u8) -> bool {
    id == FLAP_APP_ID.0
        || id == STACK_APP_ID.0
        || id == BRICK_APP_ID.0
        || id == BOO_APP_ID.0
        || id == TUNE_APP_ID.0
}
