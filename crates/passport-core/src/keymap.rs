//! Button-map and About copy. Three keys, no pointer, no Super-key chords.

pub const CHEATSHEET: &[&str] = &[
    "OK long   home",
    "UP/DN     move",
    "OK click  open",
    "UP long   desk 1",
    "DN long   desk 2",
    "system    settings",
    "phone tap NTAG213",
];

/// Product copy that used to live on the home chrome.
pub const ABOUT: &[&str] = &[
    "PassportOS",
    "FoloToy AI Passport",
    "ESP32-C3  8MB",
    "OK long  home",
    "UP/DN    move",
    "OK       open",
    "NTAG213  144B",
    "phone RF only",
    "MCU has no bus",
];

pub fn line_count() -> usize {
    CHEATSHEET.len()
}
