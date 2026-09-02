//! Idle backlight-off standby. Drives shipped `Shell` + millivolt decoder, not a copy.

use passport_core::app::{AppId, AppLifecycle};
use passport_core::board::{decode_millivolts, Key, KeyState, TYPICAL_RELEASED_MV};
use passport_core::charging_from_samples;
use passport_core::console::parse_line;
use passport_core::flap::FLAP_APP_ID;
use passport_core::input::ButtonEvent;
use passport_core::shell::{Overlay, Shell, SideEffect, IDLE_STANDBY_MS};

const PULSE: AppId = AppId(1);
const METER: AppId = AppId(2);

fn shell_with_apps() -> Shell {
    let mut sh = Shell::new();
    sh.register_app(PULSE, "pulse").unwrap();
    sh.register_app(METER, "meter").unwrap();
    sh
}

fn unplugged() -> bool {
    charging_from_samples(0, 0, Some(50), Some(50))
}

fn plugged_usb() -> bool {
    charging_from_samples(12, 3, Some(50), Some(50))
}

fn idle_released(sh: &mut Shell, ms: u32) -> passport_core::EventOutcome {
    assert_eq!(decode_millivolts(TYPICAL_RELEASED_MV), KeyState::Released);
    sh.tick_mv(TYPICAL_RELEASED_MV, ms)
}

fn enter_standby(sh: &mut Shell) -> passport_core::EventOutcome {
    assert!(!unplugged());
    let _ = sh.set_charging(unplugged());
    let bl = sh.status.brightness;
    let overlay = sh.overlay();
    let focused = sh.focused_app_name();
    let ws = sh.current_workspace();
    let out = idle_released(sh, IDLE_STANDBY_MS);
    assert_eq!(out.side, SideEffect::SetBrightness(0), "standby is PWM 0, not RTC sleep");
    assert_ne!(out.side, SideEffect::SleepLight);
    assert_ne!(out.side, SideEffect::SleepDeep);
    assert!(sh.is_standby());
    assert_eq!(sh.status.brightness, bl, "stored brightness stays the restore target");
    assert_eq!(sh.overlay(), overlay);
    assert_eq!(sh.focused_app_name(), focused);
    assert_eq!(sh.current_workspace(), ws);
    assert!(
        out.lifecycle.is_empty(),
        "blanking must not change app lifecycle, got {:?}",
        out.lifecycle
    );
    out
}

#[test]
fn unplugged_idle_timeout_blanks_without_leaving_overlay() {
    let mut sh = shell_with_apps();
    sh.enter_home();
    assert_eq!(sh.overlay(), Overlay::Launcher);
    let sel = sh.launcher().selected;
    let just_shy = idle_released(&mut sh, IDLE_STANDBY_MS - 1);
    assert!(!sh.is_standby());
    assert_ne!(just_shy.side, SideEffect::SetBrightness(0));
    enter_standby(&mut sh);
    assert_eq!(sh.overlay(), Overlay::Launcher);
    assert_eq!(sh.launcher().selected, sel);
}

#[test]
fn charging_idle_does_not_blank() {
    let mut sh = shell_with_apps();
    sh.enter_home();
    assert!(plugged_usb());
    let out = sh.set_charging(plugged_usb());
    assert_eq!(out.side, SideEffect::None);
    let out = idle_released(&mut sh, IDLE_STANDBY_MS);
    assert!(!sh.is_standby());
    assert_ne!(out.side, SideEffect::SetBrightness(0));
    assert_eq!(sh.overlay(), Overlay::Launcher);
    let out = idle_released(&mut sh, IDLE_STANDBY_MS);
    assert!(!sh.is_standby());
    assert_ne!(out.side, SideEffect::SetBrightness(0));
}

#[test]
fn plugging_in_while_blanked_restores_brightness() {
    let mut sh = shell_with_apps();
    sh.enter_home();
    let bl = sh.status.brightness;
    enter_standby(&mut sh);
    assert!(plugged_usb());
    let out = sh.set_charging(plugged_usb());
    assert!(!sh.is_standby());
    assert_eq!(out.side, SideEffect::SetBrightness(bl));
    assert_ne!(out.side, SideEffect::SetBrightness(0));
    assert_eq!(sh.overlay(), Overlay::Launcher);
    let out = idle_released(&mut sh, IDLE_STANDBY_MS);
    assert!(!sh.is_standby(), "charging must keep the panel on");
    assert_ne!(out.side, SideEffect::SetBrightness(0));
}

#[test]
fn each_key_wakes_without_stealing_ui_then_later_click_still_works() {
    for key in [Key::Up, Key::Down, Key::Ok] {
        assert_eq!(decode_millivolts(key.typical_mv()), KeyState::Down(key));
        let mut sh = shell_with_apps();
        sh.enter_home();
        let sel = sh.launcher().selected;
        let bl = sh.status.brightness;
        enter_standby(&mut sh);

        let out = sh.synth_click(key);
        assert!(!sh.is_standby(), "{key:?} must leave standby");
        assert_eq!(
            out.side,
            SideEffect::SetBrightness(bl),
            "{key:?} restore, got {:?}",
            out.side
        );
        assert_ne!(out.side, SideEffect::SetBrightness(0));
        assert_eq!(sh.overlay(), Overlay::Launcher, "{key:?} must not close launcher");
        assert_eq!(
            sh.launcher().selected, sel,
            "{key:?} wake must not move the launcher cursor"
        );
        assert!(
            out.lifecycle.is_empty(),
            "{key:?} wake must not activate apps, got {:?}",
            out.lifecycle
        );

        let moved = sh.synth_click(Key::Down);
        assert_eq!(sh.overlay(), Overlay::Launcher);
        assert_eq!(
            sh.launcher().selected,
            sel + 1,
            "after {key:?} wake, a real click must still move the launcher; side={:?}",
            moved.side
        );
    }
}

#[test]
fn wake_up_does_not_hop_utility_tiles() {
    let mut sh = shell_with_apps();
    sh.start_on_current(PULSE);
    sh.start_on_current(METER);
    assert_eq!(sh.overlay(), Overlay::None);
    assert_eq!(sh.focused_app_name(), Some("meter"));
    assert_eq!(sh.tile_count(), 2);
    enter_standby(&mut sh);

    let out = sh.synth_click(Key::Up);
    assert!(!sh.is_standby());
    assert_eq!(sh.focused_app_name(), Some("meter"), "wake UP must not focus_delta");
    assert!(
        !out.lifecycle.iter().any(|n| matches!(
            n,
            AppLifecycle::Focus(_) | AppLifecycle::Input(_, ButtonEvent::Click(Key::Up))
        )),
        "wake must not look like a tile click, got {:?}",
        out.lifecycle
    );

    let out = sh.synth_click(Key::Up);
    assert_eq!(sh.focused_app_name(), Some("pulse"));
    assert!(
        out.lifecycle.iter().any(|n| matches!(n, AppLifecycle::Focus(id) if *id == PULSE)),
        "post-wake UP click still hops tiles, got {:?}",
        out.lifecycle
    );
}

#[test]
fn wake_ok_does_not_reach_focused_flap() {
    let mut sh = shell_with_apps();
    sh.register_app(FLAP_APP_ID, "flap").unwrap();
    sh.enter_home();
    sh.apply_command(parse_line("activate flap").unwrap());
    assert_eq!(sh.overlay(), Overlay::None);
    assert_eq!(sh.focused_app_name(), Some("flap"));
    enter_standby(&mut sh);

    let out = sh.synth_click(Key::Ok);
    assert!(!sh.is_standby());
    assert_eq!(sh.overlay(), Overlay::None);
    assert_eq!(sh.focused_app_name(), Some("flap"));
    assert!(
        !out.lifecycle.iter().any(|n| matches!(
            n,
            AppLifecycle::Input(id, _) if *id == FLAP_APP_ID
        )),
        "wake OK must not flap, got {:?}",
        out.lifecycle
    );

    let out = sh.synth_click(Key::Ok);
    assert!(
        out.lifecycle.iter().any(|n| matches!(
            n,
            AppLifecycle::Input(id, ButtonEvent::Click(Key::Ok) | ButtonEvent::Press(Key::Ok))
                if *id == FLAP_APP_ID
        )),
        "post-wake OK still reaches flap, got {:?}",
        out.lifecycle
    );
}
