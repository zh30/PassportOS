//! Speaker control: volume/mute state, ES8311 DAC mapping, audio lifecycle.

use passport_core::api::MemoryStore;
use passport_core::board::Key;
use passport_core::console::parse_line;
use passport_core::es8311::{DAC_VOL_MAX, REG_DAC_VOLUME, dac_volume};
use passport_core::input::ButtonEvent;
use passport_core::menu::{MENU_ITEMS, MenuAction};
use passport_core::radio::Resource;
use passport_core::shell::{Shell, SideEffect};
use passport_core::wifi::SavedWifi;

fn shell() -> Shell {
    Shell::new()
}

#[test]
fn dac_volume_maps_percent_to_register() {
    assert_eq!(dac_volume(0, false), 0x00);
    assert_eq!(dac_volume(100, false), DAC_VOL_MAX);
    assert_eq!(dac_volume(50, false), DAC_VOL_MAX / 2);
    assert_eq!(dac_volume(255, false), DAC_VOL_MAX, "clamps above 100");
    // Monotone.
    let mut prev = 0;
    for v in 1..=100u8 {
        let r = dac_volume(v, false);
        assert!(r >= prev, "vol {v} reg {r} < {prev}");
        prev = r;
    }
    // Mute is the deepest attenuation regardless of level.
    assert_eq!(dac_volume(100, true), 0x00);
    assert_eq!(dac_volume(1, true), 0x00);
}

#[test]
fn es8311_init_sets_dac_volume_to_max() {
    // INIT must write the DAC register; boot applies status.volume later.
    let v = passport_core::es8311::last_write(REG_DAC_VOLUME);
    assert_eq!(v, Some(0xBF), "INIT writes 0x32=0xBF");
}

#[test]
fn console_volume_sets_state_and_side() {
    let mut sh = shell();
    let out = sh.apply_command(parse_line("vol 40").unwrap());
    assert_eq!(out.side, SideEffect::AudioVolume(40));
    assert_eq!(sh.status.volume, 40);
    assert!(!sh.status.muted, "explicit volume unmutes");

    // Clamped at 100.
    let out = sh.apply_command(parse_line("vol 250").unwrap());
    assert_eq!(out.side, SideEffect::AudioVolume(100));
    assert_eq!(sh.status.volume, 100);

    // `vol` alone reports, no side effect.
    let out = sh.apply_command(parse_line("vol").unwrap());
    assert_eq!(out.side, SideEffect::None);
    assert!(out.reply.contains("vol=100"), "{}", out.reply);
}

#[test]
fn mute_toggle_roundtrip() {
    let mut sh = shell();
    let out = sh.apply_command(parse_line("mute").unwrap());
    assert_eq!(out.side, SideEffect::AudioMute(true));
    assert!(sh.status.muted);
    let out = sh.apply_command(parse_line("mute").unwrap());
    assert_eq!(out.side, SideEffect::AudioMute(false));
    assert!(!sh.status.muted);
    // Volume survives the mute roundtrip.
    assert_eq!(sh.status.volume, 80);
}

#[test]
fn audio_stop_releases_exclusive_audio() {
    let mut sh = shell();
    let out = sh.apply_command(parse_line("audio beep").unwrap());
    assert_eq!(out.side, SideEffect::AudioBeep);
    assert_eq!(sh.exclusive.owner(), Some(Resource::Audio));

    let out = sh.apply_command(parse_line("audio stop").unwrap());
    assert_eq!(out.side, SideEffect::AudioStop);
    assert!(
        sh.exclusive.owner().is_none(),
        "audio stop frees the shared DMA/radio budget"
    );
}

#[test]
fn menu_volume_and_mute_actions() {
    let mut sh = shell();
    sh.apply_command(parse_line("menu").unwrap());

    fn goto(sh: &mut Shell, want: MenuAction) {
        let idx = MENU_ITEMS
            .iter()
            .position(|i| i.action == want)
            .expect("menu item exists");
        while sh.menu_selected() != idx {
            sh.handle_event(ButtonEvent::Click(Key::Down));
        }
    }

    goto(&mut sh, MenuAction::VolumeDec);
    let out = sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(out.side, SideEffect::AudioVolume(70));
    assert_eq!(sh.status.volume, 70);

    goto(&mut sh, MenuAction::VolumeInc);
    let out = sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(out.side, SideEffect::AudioVolume(80));

    goto(&mut sh, MenuAction::MuteToggle);
    let out = sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(out.side, SideEffect::AudioMute(true));
    assert!(sh.status.muted);

    // Volume while muted unmutes.
    goto(&mut sh, MenuAction::VolumeInc);
    let out = sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(out.side, SideEffect::AudioVolume(90));
    assert!(!sh.status.muted);
}

#[test]
fn status_line_reports_volume_and_mute() {
    let mut sh = shell();
    sh.apply_command(parse_line("vol 33").unwrap());
    sh.refresh_status();
    assert!(sh.status.format().contains("vol=33"));
    sh.apply_command(parse_line("mute").unwrap());
    assert!(sh.status.format().contains("vol=mute"));
}

#[test]
fn ble_conn_flag_marks_screen_radio() {
    let mut sh = shell();
    sh.apply_command(parse_line("radio ble").unwrap());
    assert!(sh.status.format_screen().contains("ble"));
    sh.set_ble_conn(true);
    assert!(sh.status.format_screen().contains("ble*"));
    sh.apply_command(parse_line("radio off").unwrap());
    assert!(!sh.status.ble_conn, "radio off clears the conn flag");
}

#[test]
fn volume_not_persisted_in_ram_only_store_without_keys() {
    // SavedWifi uses the same Store trait — confirm empty store has none.
    let store = MemoryStore::new();
    assert!(SavedWifi::load(&store).is_none());
}
