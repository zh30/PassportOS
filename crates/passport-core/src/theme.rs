//! Light / dark palettes. Large fills never use 0x0000 (黑屏) or 0xFFFF (花屏).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
}

impl Theme {
    pub const fn toggle(self) -> Self {
        match self {
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::Dark,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Light => "light",
        }
    }

    pub const fn is_light(self) -> bool {
        matches!(self, Theme::Light)
    }

    pub const fn palette(self) -> Palette {
        match self {
            Theme::Dark => Palette::DARK,
            Theme::Light => Palette::LIGHT,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::Dark
    }
}

/// Semantic colours, Apple grouped-table roles mapped onto this ST7789.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// Page (systemGroupedBackground).
    pub bg: u16,
    /// Inset group fill (secondarySystemGroupedBackground).
    pub grouped: u16,
    /// Highlighted row.
    pub grouped_sel: u16,
    /// Primary label.
    pub label: u16,
    /// Secondary label / chevron.
    pub secondary: u16,
    /// systemBlue.
    pub accent: u16,
    pub separator: u16,
    pub ok: u16,
    pub low: u16,
    pub icon_pulse: u16,
    pub icon_tap: u16,
    pub icon_system: u16,
}

impl Palette {
    /// Dark: large fills are the proven invert-on field (`0x10A2` / `0x2945`).
    pub const DARK: Self = Self {
        bg: 0x10A2,
        grouped: 0x2945,
        grouped_sel: 0x39C7,
        label: 0xEF7D,
        secondary: 0x8410,
        accent: 0x07FD,
        separator: 0x4208,
        ok: 0x07E0,
        low: 0xF800,
        icon_pulse: 0x07FD,
        icon_tap: 0x07E0,
        icon_system: 0x8410,
    };

    /// Light: off-white cells (`0xEF7D`, already shown as glyphs). Page gray is
    /// banded on paint — never a single 240×320 white RAMWR.
    pub const LIGHT: Self = Self {
        bg: 0xDEFB,
        grouped: 0xEF7D,
        grouped_sel: 0xD69A,
        label: 0x2124,
        secondary: 0x8410,
        accent: 0x03DF,
        separator: 0xC639,
        ok: 0x07E0,
        low: 0xF800,
        icon_pulse: 0x03DF,
        icon_tap: 0x07E0,
        icon_system: 0x8410,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_large_fills_are_proven_field() {
        assert_eq!(Palette::DARK.bg, 0x10A2);
        assert_eq!(Palette::DARK.grouped, 0x2945);
        assert_ne!(Palette::DARK.bg, 0x0000);
        assert_ne!(Palette::DARK.label, 0x0000);
    }

    #[test]
    fn light_avoids_pure_black_and_white_fills() {
        assert_ne!(Palette::LIGHT.bg, 0x0000);
        assert_ne!(Palette::LIGHT.bg, 0xFFFF);
        assert_ne!(Palette::LIGHT.grouped, 0x0000);
        assert_ne!(Palette::LIGHT.grouped, 0xFFFF);
        assert_ne!(Palette::LIGHT.label, 0x0000);
    }

    #[test]
    fn toggle_roundtrips() {
        assert_eq!(Theme::Dark.toggle(), Theme::Light);
        assert_eq!(Theme::Light.toggle(), Theme::Dark);
        assert_eq!(Theme::Dark.as_str(), "dark");
        assert_eq!(Theme::Light.as_str(), "light");
    }
}
