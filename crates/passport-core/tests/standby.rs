//! Idle backlight-off standby. Drives shipped `Shell` + millivolt decoder, not a copy.

use passport_core::app::{AppId, AppLifecycle};
use passport_core::board::{
    decode_millivolts, Key, KeyState, INPUT_TICK_MS, TYPICAL_RELEASED_MV,
};
use passport_core::charging_from_samples;
use passport_core::console::parse_line;
use passport_core::flap::FLAP_APP_ID;
use passport_core::input::{ButtonEvent, DEBOUNCE_MS, RELEASE_DEBOUNCE_MS};
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

/// Firmware cadence: 5 ms ADC samples, then a 1 ms post-SPI edge, then settle.
fn press_until_down(sh: &mut Shell, key: Key) {
    assert_eq!(decode_millivolts(key.typical_mv()), KeyState::Down(key));
    let ticks = (DEBOUNCE_MS / INPUT_TICK_MS) + 1;
    for _ in 0..ticks {
        let _ = sh.tick_mv(key.typical_mv(), INPUT_TICK_MS);
    }
    assert_eq!(sh.decoder().current(), KeyState::Down(key));
}

fn firmware_release_edge(sh: &mut Shell) -> (passport_core::EventOutcome, passport_core::EventOutcome) {
    // Post-SPI sample is often 1 ms — shorter than RELEASE_DEBOUNCE_MS.
    let early = sh.tick_mv(TYPICAL_RELEASED_MV, 1);
    let settled = sh.tick_mv(TYPICAL_RELEASED_MV, RELEASE_DEBOUNCE_MS);
    (early, settled)
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

#[test]
fn wake_release_at_firmware_cadence_does_not_steal_launcher() {
    for key in [Key::Up, Key::Down, Key::Ok] {
        let mut sh = shell_with_apps();
        sh.enter_home();
        let sel = sh.launcher().selected;
        enter_standby(&mut sh);
        press_until_down(&mut sh, key);
        assert!(!sh.is_standby(), "{key:?} press must wake");
        assert_eq!(sh.overlay(), Overlay::Launcher);
        assert_eq!(sh.launcher().selected, sel);

        let (early, settled) = firmware_release_edge(&mut sh);
        assert_eq!(
            sh.decoder().current(),
            KeyState::Released,
            "{key:?} after settle"
        );
        assert!(
            early.lifecycle.is_empty() && settled.lifecycle.is_empty(),
            "{key:?} 1 ms then {RELEASE_DEBOUNCE_MS} ms Released must not activate, early={:?} settled={:?}",
            early.lifecycle,
            settled.lifecycle
        );
        assert_eq!(sh.overlay(), Overlay::Launcher, "{key:?}");
        assert_eq!(
            sh.launcher().selected, sel,
            "{key:?} Click after 1 ms Released must still be swallowed"
        );

        let moved = sh.synth_click(Key::Down);
        assert_eq!(
            sh.launcher().selected,
            sel + 1,
            "after firmware-cadence {key:?} wake, later click must move; side={:?}",
            moved.side
        );
    }
}

#[test]
fn wake_ok_at_firmware_cadence_does_not_reach_flap() {
    let mut sh = shell_with_apps();
    sh.register_app(FLAP_APP_ID, "flap").unwrap();
    sh.enter_home();
    sh.apply_command(parse_line("activate flap").unwrap());
    enter_standby(&mut sh);
    press_until_down(&mut sh, Key::Ok);
    assert!(!sh.is_standby());
    assert_eq!(sh.focused_app_name(), Some("flap"));

    let (early, settled) = firmware_release_edge(&mut sh);
    for (label, out) in [("dt=1", &early), ("dt=5", &settled)] {
        assert!(
            !out.lifecycle.iter().any(|n| matches!(
                n,
                AppLifecycle::Input(id, _) if *id == FLAP_APP_ID
            )),
            "wake OK {label} must not flap, got {:?}",
            out.lifecycle
        );
    }
    assert_eq!(sh.overlay(), Overlay::None);
    assert_eq!(sh.focused_app_name(), Some("flap"));

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
