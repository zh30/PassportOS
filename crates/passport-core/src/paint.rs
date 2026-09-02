//! What to redraw. Full-panel SPI fills flash on ST7789; selection changes
//! only need the two affected cards. Live animation is a separate generation
//! so a game tick never wipes the playfield.

use crate::flap::FLAP_APP_ID;
use crate::stack::STACK_APP_ID;
use crate::brick::BRICK_APP_ID;
use crate::shell::{Overlay, Shell};
use crate::status::RadioMode;
use crate::theme::Theme;
use crate::wifi::WifiPhase;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameSig {
    pub overlay: Overlay,
    pub launcher_sel: usize,
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
    pub ime_idx: u8,
    pub ime_len: u8,
    pub ime_shift: bool,
}

impl FrameSig {
    pub fn capture(shell: &Shell) -> Self {
        Self {
            overlay: shell.overlay(),
            launcher_sel: shell.launcher().selected,
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
            ime_idx: shell.wifi().ime().cursor() as u8,
            ime_len: shell.wifi().ime().len() as u8,
            ime_shift: shell.wifi().ime().shift(),
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
        if p.overlay != now.overlay || p.focused != now.focused || p.theme != now.theme {
            return Self::full(now);
        }
        let status = p.soc != now.soc
            || p.charging != now.charging
            || p.radio != now.radio
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
            Overlay::Wifi if p.wifi_phase != now.wifi_phase => Self::full(now),
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

fn is_live_game(id: u8) -> bool {
    id == FLAP_APP_ID.0 || id == STACK_APP_ID.0 || id == BRICK_APP_ID.0
}
