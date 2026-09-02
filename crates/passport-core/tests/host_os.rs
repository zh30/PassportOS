//! Host tests against the *shipped* decoder, tiler, launcher, workspaces, and app registry.
//! These call `passport_core` public functions — they do not reimplement the millivolt windows.

use passport_core::app::{AppId, AppLifecycle};
use passport_core::brick::BRICK_APP_ID;
use passport_core::flap::FLAP_APP_ID;
use passport_core::stack::STACK_APP_ID;
use passport_core::board::{
    adc_bar_width, battery_poll_due, decode_millivolts, idle_telemetry_due, Key, KeyState,
    FLASH_APP_OFFSET, FLASH_APP_SIZE, FLASH_CARDID_OFFSET, FLASH_KV_OFFSET, FLASH_KV_SIZE,
    IDLE_TELEMETRY_PERIOD_TICKS, LCD_H, TYPICAL_DOWN_MV, TYPICAL_OK_MV, TYPICAL_RELEASED_MV,
    TYPICAL_UP_MV,
};
use passport_core::compositor::{layout_tiles, STATUS_BAR_H};
use passport_core::console::parse_line;
use passport_core::input::{ButtonDecoder, ButtonEvent, LONG_PRESS_MS};
use passport_core::radio::{ExclusiveManager, Resource};
use passport_core::menu::{MenuAction, MENU_ITEMS};
use passport_core::paint::{FrameSig, PaintPlan};
use passport_core::shell::{Overlay, Shell, SideEffect};
use passport_core::theme::Theme;
use passport_core::Command;

const PULSE: AppId = AppId(1);
const METER: AppId = AppId(2);

fn shell_with_apps() -> Shell {
    let mut sh = Shell::new();
    sh.register_app(PULSE, "pulse").unwrap();
    sh.register_app(METER, "meter").unwrap();
    sh
}

#[test]
fn home_is_launcher_with_sample_app() {
    let mut sh = shell_with_apps();
    sh.enter_home();
    assert_eq!(sh.overlay(), Overlay::Launcher);
    assert!(sh.launcher().is_open());
    let names = sh.launcher_names();
    assert!(
        names.iter().any(|n| *n == "pulse"),
        "home launcher must list pulse, got {names:?}"
    );
    let screen = sh.status.format_screen();
    assert!(
        screen.len() <= 40,
        "LCD status must fit 40 columns, got {} {:?}",
        screen.len(),
        screen
    );
    assert!(screen.contains("ws"), "{screen}");
    // battery + radio always present on the LCD line
    assert!(
        screen.contains("--") || screen.as_bytes().iter().any(|b| *b == b'%'),
        "screen status missing battery: {screen}"
    );
    assert!(
        screen.contains("off") || screen.contains("wifi") || screen.contains("ble"),
        "screen status missing radio: {screen}"
    );
    assert!(
        screen.contains("--:--") || screen.as_str().as_bytes().windows(5).any(|w| w[2] == b':'),
        "screen status missing clock: {screen}"
    );
}

#[test]
fn adc_bar_is_stable_voltage_meter_not_a_loop() {
    let max = 200;
    assert_eq!(adc_bar_width(0, max), 0);
    assert_eq!(adc_bar_width(TYPICAL_RELEASED_MV, max), max);
    assert_eq!(adc_bar_width(TYPICAL_RELEASED_MV + 100, max), max);
    let mid = adc_bar_width(TYPICAL_RELEASED_MV / 2, max);
    assert!(mid > 0 && mid < max, "mid={mid}");
    // Same millivolts always yield the same width — no wrapping phase.
    assert_eq!(
        adc_bar_width(TYPICAL_RELEASED_MV, max),
        adc_bar_width(TYPICAL_RELEASED_MV, max)
    );
    assert!(adc_bar_width(TYPICAL_OK_MV, max) < adc_bar_width(TYPICAL_RELEASED_MV, max));
}

#[test]
fn kv_flash_sits_between_factory_and_cardid() {
    assert!(FLASH_KV_OFFSET >= FLASH_APP_OFFSET + FLASH_APP_SIZE);
    assert!(FLASH_KV_OFFSET + FLASH_KV_SIZE <= FLASH_CARDID_OFFSET);
    assert_eq!(FLASH_KV_SIZE, 0x1000);
}

#[test]
fn millivolts_map_official_windows() {
    // Official windows {[0,150),[150,447),[447,1900)}; typical ladder 0 / ~300 / ~595 / ~3300.
    assert_eq!(decode_millivolts(TYPICAL_UP_MV), KeyState::Down(Key::Up));
    assert_eq!(decode_millivolts(0), KeyState::Down(Key::Up));
    assert_eq!(decode_millivolts(149), KeyState::Down(Key::Up));
    assert_eq!(decode_millivolts(TYPICAL_DOWN_MV), KeyState::Down(Key::Down));
    assert_eq!(decode_millivolts(150), KeyState::Down(Key::Down));
    assert_eq!(decode_millivolts(446), KeyState::Down(Key::Down));
    assert_eq!(decode_millivolts(TYPICAL_OK_MV), KeyState::Down(Key::Ok));
    assert_eq!(decode_millivolts(447), KeyState::Down(Key::Ok));
    assert_eq!(decode_millivolts(1899), KeyState::Down(Key::Ok));
    assert_eq!(decode_millivolts(TYPICAL_RELEASED_MV), KeyState::Released);
    assert_eq!(decode_millivolts(1900), KeyState::Released);
    assert_eq!(decode_millivolts(3300), KeyState::Released);
}

#[test]
fn click_vs_long_press_uses_shipped_decoder() {
    let mut dec = ButtonDecoder::new();

    // Click: hold UP ~100 ms then release — Press + Click, no LongPress.
    let mut events = Vec::new();
    for _ in 0..6 {
        events.extend(dec.feed(TYPICAL_UP_MV, 20));
    }
    for _ in 0..6 {
        events.extend(dec.feed(TYPICAL_RELEASED_MV, 20));
    }
    assert!(
        events.contains(&ButtonEvent::Press(Key::Up)),
        "expected Press from shipped decoder, got {events:?}"
    );
    assert!(
        events.contains(&ButtonEvent::Click(Key::Up)),
        "expected Click from shipped decoder, got {events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(e, ButtonEvent::LongPress(_))),
        "short hold must not long-press: {events:?}"
    );

    // Long-press: hold DOWN past LONG_PRESS_MS — Press + LongPress, no Click.
    let mut dec = ButtonDecoder::new();
    let mut events = Vec::new();
    let ticks = (LONG_PRESS_MS / 20) + 6;
    for _ in 0..ticks {
        events.extend(dec.feed(TYPICAL_DOWN_MV, 20));
    }
    for _ in 0..6 {
        events.extend(dec.feed(TYPICAL_RELEASED_MV, 20));
    }
    assert!(events.contains(&ButtonEvent::Press(Key::Down)));
    assert!(
        events.contains(&ButtonEvent::LongPress(Key::Down)),
        "expected LongPress from shipped decoder, got {events:?}"
    );
    assert!(
        !events.contains(&ButtonEvent::Click(Key::Down)),
        "long-press must suppress Click: {events:?}"
    );
}

#[test]
fn rapid_ok_taps_each_emit_press_and_click() {
    // Mash OK: press, a one-sample Released gap, press again.
    // Symmetric 30 ms release debounce used to keep the decoder in Down(Ok)
    // so the second tap produced nothing.
    let mut dec = ButtonDecoder::new();
    let mut events = Vec::new();
    for _ in 0..3 {
        events.extend(dec.feed(TYPICAL_OK_MV, 20));
    }
    events.extend(dec.feed(TYPICAL_RELEASED_MV, 20));
    for _ in 0..3 {
        events.extend(dec.feed(TYPICAL_OK_MV, 20));
    }
    events.extend(dec.feed(TYPICAL_RELEASED_MV, 20));
    let presses = events
        .iter()
        .filter(|e| matches!(e, ButtonEvent::Press(Key::Ok)))
        .count();
    let clicks = events
        .iter()
        .filter(|e| matches!(e, ButtonEvent::Click(Key::Ok)))
        .count();
    assert_eq!(presses, 2, "expected two OK presses, got {events:?}");
    assert_eq!(clicks, 2, "expected two OK clicks, got {events:?}");
}

#[test]
fn rapid_ok_taps_at_input_cadence() {
    use passport_core::INPUT_TICK_MS;
    let mut dec = ButtonDecoder::new();
    let mut events = Vec::new();
    let press_samples = (30 / INPUT_TICK_MS) + 1;
    for _ in 0..press_samples {
        events.extend(dec.feed(TYPICAL_OK_MV, INPUT_TICK_MS));
    }
    events.extend(dec.feed(TYPICAL_RELEASED_MV, INPUT_TICK_MS));
    for _ in 0..press_samples {
        events.extend(dec.feed(TYPICAL_OK_MV, INPUT_TICK_MS));
    }
    events.extend(dec.feed(TYPICAL_RELEASED_MV, INPUT_TICK_MS));
    let presses = events
        .iter()
        .filter(|e| matches!(e, ButtonEvent::Press(Key::Ok)))
        .count();
    assert_eq!(presses, 2, "5 ms cadence must see both taps, got {events:?}");
}

fn feed_seq(dec: &mut ButtonDecoder, samples: &[u16]) -> Vec<ButtonEvent> {
    let mut events = Vec::new();
    for mv in samples {
        events.extend(dec.feed(*mv, 20));
    }
    events
}

#[test]
fn ladder_ramp_does_not_click_intermediate_ok() {
    // Physical UP: 3300 → through OK (~595) and DOWN (~300) → 0, then release.
    let mut dec = ButtonDecoder::new();
    let events = feed_seq(
        &mut dec,
        &[
            TYPICAL_RELEASED_MV,
            900,
            900,
            900,
            TYPICAL_DOWN_MV,
            TYPICAL_DOWN_MV,
            TYPICAL_UP_MV,
            TYPICAL_UP_MV,
            TYPICAL_UP_MV,
            TYPICAL_UP_MV,
            TYPICAL_RELEASED_MV,
            TYPICAL_RELEASED_MV,
            TYPICAL_RELEASED_MV,
        ],
    );
    assert!(
        !events.contains(&ButtonEvent::Click(Key::Ok)),
        "UP press must not click OK while walking the ladder: {events:?}"
    );
    assert!(
        !events.contains(&ButtonEvent::Click(Key::Down)),
        "UP press must not click DOWN on the way through: {events:?}"
    );
    assert!(
        events.contains(&ButtonEvent::Click(Key::Up)),
        "UP press must click UP on release: {events:?}"
    );

    // Physical DOWN: 3300 → through OK → 300, then release.
    let mut dec = ButtonDecoder::new();
    let events = feed_seq(
        &mut dec,
        &[
            TYPICAL_RELEASED_MV,
            900,
            900,
            900,
            TYPICAL_DOWN_MV,
            TYPICAL_DOWN_MV,
            TYPICAL_DOWN_MV,
            TYPICAL_DOWN_MV,
            TYPICAL_RELEASED_MV,
            TYPICAL_RELEASED_MV,
            TYPICAL_RELEASED_MV,
        ],
    );
    assert!(
        !events.contains(&ButtonEvent::Click(Key::Ok)),
        "DOWN press must not click OK while walking the ladder: {events:?}"
    );
    assert!(
        events.contains(&ButtonEvent::Click(Key::Down)),
        "DOWN press must click DOWN on release: {events:?}"
    );
}

#[test]
fn ladder_ramp_on_home_navigates_instead_of_opening_pulse() {
    let mut sh = shell_with_apps();
    sh.enter_home();
    assert_eq!(sh.launcher().selected, 0);
    let n = sh.launcher_names().len();
    assert!(n >= 3, "home lists pulse/meter/system, got {n}");
    assert!(
        !sh.launcher_names().iter().any(|n| *n == "keys"),
        "keys belongs in the system menu, not the launcher"
    );

    // Same millivolt walk as a physical UP press.
    for mv in [
        900u16, 900, 900, TYPICAL_DOWN_MV, TYPICAL_DOWN_MV, TYPICAL_UP_MV, TYPICAL_UP_MV,
        TYPICAL_UP_MV, TYPICAL_UP_MV,
    ] {
        sh.tick_mv(mv, 20);
    }
    for _ in 0..5 {
        sh.tick_mv(TYPICAL_RELEASED_MV, 20);
    }
    assert_eq!(
        sh.overlay(),
        Overlay::Launcher,
        "UP must not activate the selected app"
    );
    assert_eq!(
        sh.launcher().selected,
        n - 1,
        "UP click wraps to the last launcher item"
    );

    // Physical DOWN from the wrapped item should move toward pulse, not activate.
    let start = sh.launcher().selected;
    for mv in [900u16, 900, 900, TYPICAL_DOWN_MV, TYPICAL_DOWN_MV, TYPICAL_DOWN_MV, TYPICAL_DOWN_MV]
    {
        sh.tick_mv(mv, 20);
    }
    for _ in 0..5 {
        sh.tick_mv(TYPICAL_RELEASED_MV, 20);
    }
    assert_eq!(sh.overlay(), Overlay::Launcher);
    assert_eq!(sh.launcher().selected, (start + 1) % n);
}

#[test]
fn launcher_open_filter_activate() {
    let mut sh = shell_with_apps();
    assert!(!sh.launcher().is_open());

    // OK long-press is the Super-key analogue: opens the unified launcher.
    let _ = sh.synth_long(Key::Ok);
    assert_eq!(sh.overlay(), Overlay::Launcher);
    assert!(sh.launcher().is_open());
    let names = sh.launcher_names();
    assert!(
        names.iter().any(|n| *n == "pulse"),
        "launcher must list the sample app, got {names:?}"
    );
    assert!(names.iter().any(|n| *n == "meter"));
    assert!(names.iter().any(|n| *n == "system"));

    // Filter is a shipped launcher operation (prefix match).
    let out = sh.apply_command(parse_line("filter pu").unwrap());
    assert!(
        out.reply.contains("pulse"),
        "filter reply should list pulse: {}",
        out.reply
    );
    assert!(
        !out.reply.contains("meter"),
        "filter 'pu' must hide meter: {}",
        out.reply
    );
    let visible = sh.launcher_names();
    assert_eq!(&visible[..], &["pulse"]);

    // Activate the filtered selection through the same activate path the console uses.
    let out = sh.apply_command(Command::ActivateSelected);
    assert!(
        out.lifecycle.iter().any(|e| *e == AppLifecycle::Start(PULSE)),
        "activate must Start pulse via registry, got {:?}",
        out.lifecycle
    );
    assert!(
        out.lifecycle.iter().any(|e| *e == AppLifecycle::Focus(PULSE)),
        "activate must Focus pulse, got {:?}",
        out.lifecycle
    );
    assert_eq!(sh.overlay(), Overlay::None);
    assert_eq!(sh.focused_app_name(), Some("pulse"));
    let pulse = sh.registry.get(PULSE).unwrap();
    assert!(pulse.running && pulse.focused);
}

#[test]
fn tile_split_vs_single() {
    let single = layout_tiles(1);
    assert_eq!(single.tiles.len(), 1);
    assert_eq!(single.status.h, STATUS_BAR_H);
    assert_eq!(single.tiles[0].y, STATUS_BAR_H);
    assert_eq!(single.tiles[0].y + single.tiles[0].h, 320);
    assert_eq!(single.tiles[0].w, 240);
    assert!(single.is_non_overlapping());
    assert!(!single.tiles[0].overlaps(single.status));

    let split = layout_tiles(2);
    assert_eq!(split.tiles.len(), 2);
    assert!(split.is_non_overlapping(), "tiles must not overlap");
    assert_eq!(split.tiles[0].y, STATUS_BAR_H);
    assert_eq!(split.tiles[0].bottom(), split.tiles[1].y);
    assert_eq!(split.tiles[1].bottom(), 320);
    assert!(!split.tiles[0].overlaps(split.tiles[1]));
    assert!(!split.tiles[0].overlaps(split.status));
    assert!(!split.tiles[1].overlaps(split.status));

    let empty = layout_tiles(0);
    assert!(empty.tiles.is_empty());
}

#[test]
fn workspace_switch_and_app_lifecycle() {
    let mut sh = shell_with_apps();
    assert_eq!(sh.current_workspace(), 0);
    assert_eq!(sh.tile_count(), 0);

    let notes = sh.start_on_current(PULSE);
    assert!(notes.contains(&AppLifecycle::Start(PULSE)));
    assert!(notes.contains(&AppLifecycle::Focus(PULSE)));
    assert_eq!(sh.tile_count(), 1);
    assert_eq!(sh.layout().tiles.len(), 1);

    let notes = sh.start_on_current(METER);
    assert!(notes.contains(&AppLifecycle::Start(METER)));
    assert!(notes.contains(&AppLifecycle::Focus(METER)));
    assert!(notes.contains(&AppLifecycle::Blur(PULSE)));
    assert_eq!(sh.tile_count(), 2, "two apps on one workspace split the surface");
    assert_eq!(sh.layout().tiles.len(), 2);

    // UP long-press stays on workspace 0; DOWN long-press switches to workspace 1.
    let notes = sh.synth_long(Key::Down);
    assert_eq!(sh.current_workspace(), 1);
    assert!(
        notes.lifecycle.iter().any(|e| matches!(e, AppLifecycle::Blur(_))),
        "leaving a workspace blurs the focused app: {:?}",
        notes.lifecycle
    );
    assert_eq!(sh.tile_count(), 0);
    assert_eq!(sh.focused_app_name(), None);

    let notes = sh.synth_long(Key::Up);
    assert_eq!(sh.current_workspace(), 0);
    assert!(
        notes.lifecycle.contains(&AppLifecycle::Focus(METER)),
        "returning to ws0 restores focus: {:?}",
        notes.lifecycle
    );
    assert_eq!(sh.focused_app_name(), Some("meter"));

    // Stop via registry.
    let notes = sh.registry.stop(METER);
    assert!(notes.contains(&AppLifecycle::Blur(METER)) || !sh.registry.get(METER).unwrap().focused);
    assert!(notes.contains(&AppLifecycle::Stop(METER)));
    assert!(!sh.registry.get(METER).unwrap().running);
}

#[test]
fn console_key_inject_uses_decoder_path() {
    let mut sh = shell_with_apps();
    // `key ok long` is implemented as millivolt ticks, not a fake overlay poke.
    let cmd = parse_line("key ok long").unwrap();
    assert_eq!(cmd, Command::KeyLong(Key::Ok));
    let out = sh.apply_command(cmd);
    assert_eq!(sh.overlay(), Overlay::Launcher);
    assert!(out.reply.contains("launcher") || sh.launcher().is_open());

    let cmd = parse_line("key mv 0 5").unwrap();
    sh.apply_command(cmd);
    assert_eq!(
        sh.decoder().current(),
        KeyState::Down(Key::Up),
        "key mv 0 must go through decode_millivolts"
    );
}

#[test]
fn status_bar_includes_battery_and_radio() {
    let mut sh = shell_with_apps();
    sh.status.battery_soc = Some(87);
    sh.status.battery_mv = Some(4100);
    sh.refresh_status();
    let line = sh.status.format();
    assert!(line.contains("bat=87%(4100mV)"), "{line}");
    assert!(line.contains("radio=off"), "{line}");
    let screen = sh.status.format_screen();
    assert!(screen.len() <= 40, "{screen}");
    assert!(screen.contains("87%"), "{screen}");
    assert!(screen.contains("off"), "{screen}");
    sh.apply_command(parse_line("radio wifi").unwrap());
    let line = sh.status.format();
    assert!(line.contains("radio=wifi"), "{line}");
}

#[test]
fn charging_follows_usb_sof_or_rising_soc() {
    use passport_core::charging_from_samples;
    assert!(!charging_from_samples(0, 0, Some(50), Some(50)));
    assert!(charging_from_samples(12, 3, Some(50), Some(50)));
    assert!(!charging_from_samples(0, 40, Some(50), Some(50)));
    assert!(charging_from_samples(0, 0, Some(51), Some(50)));
    let mut sh = shell_with_apps();
    sh.status.battery_soc = Some(90);
    sh.status.charging = true;
    sh.refresh_status();
    let line = sh.status.format();
    assert!(line.contains("chg"), "{line}");
    let screen = sh.status.format_screen();
    assert!(screen.contains("90%+"), "{screen}");
}

#[test]
fn exclusive_radio_audio_cannot_share() {
    let mut ex = ExclusiveManager::new();
    assert!(ex.can_share_with(Resource::Wifi));
    let displaced = ex.acquire(Resource::Wifi);
    assert_eq!(displaced, None);
    assert!(!ex.can_share_with(Resource::Ble));
    assert!(!ex.can_share_with(Resource::Audio));
    let displaced = ex.acquire(Resource::Audio);
    assert_eq!(displaced, Some(Resource::Wifi));
    assert_eq!(ex.owner(), Some(Resource::Audio));
}

#[test]
fn console_drive_launcher_workspace_same_path_as_usb() {
    // Same parse_line + apply_command the USB console uses.
    let mut sh = shell_with_apps();
    let out = sh.apply_command(parse_line("launcher").unwrap());
    println!("DRIVE {out:?}");
    assert!(
        out.reply.contains("pulse"),
        "launcher must list sample app pulse: {}",
        out.reply
    );
    let out = sh.apply_command(parse_line("activate pulse").unwrap());
    println!("DRIVE activate {}", out.reply);
    assert_eq!(sh.focused_app_name(), Some("pulse"));
    assert!(
        out.lifecycle.iter().any(|e| *e == AppLifecycle::Focus(PULSE)),
        "activating pulse must change focus: {:?}",
        out.lifecycle
    );
    let out = sh.apply_command(parse_line("workspace 1").unwrap());
    println!("DRIVE {}", out.reply);
    assert_eq!(sh.current_workspace(), 1);
    assert!(out.reply.contains("workspace 1"));
    let status = sh.apply_command(parse_line("status").unwrap());
    println!("DRIVE {}", status.reply);
    assert!(status.reply.contains("radio="));
    assert!(status.reply.contains("bat="));
}

#[test]
fn system_menu_and_cheatsheet_from_launcher() {
    let mut sh = shell_with_apps();
    sh.apply_command(parse_line("launcher").unwrap());
    sh.apply_command(parse_line("activate system").unwrap());
    assert_eq!(sh.overlay(), Overlay::System);
    assert!(
        !sh.launcher_names().iter().any(|n| *n == "keys"),
        "keys is a menu item, not a launcher card"
    );

    select_menu_action(&mut sh, MenuAction::Keys);
    let out = sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(out.side, SideEffect::None);
    assert_eq!(sh.overlay(), Overlay::Keys);
    assert_eq!(passport_core::keymap::line_count(), 7);

    sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(sh.overlay(), Overlay::System, "keys click returns to menu");

    select_menu_action(&mut sh, MenuAction::About);
    sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(sh.overlay(), Overlay::About);
    assert!(passport_core::keymap::ABOUT.iter().any(|l| l.contains("PassportOS")));
    assert!(passport_core::keymap::ABOUT.iter().any(|l| l.contains("NTAG213")));

    sh.handle_event(ButtonEvent::Click(Key::Down));
    assert_eq!(sh.overlay(), Overlay::System);

    select_menu_action(&mut sh, MenuAction::Close);
    sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(sh.overlay(), Overlay::Launcher, "close returns home");

    assert!(MENU_ITEMS.iter().any(|i| i.action == MenuAction::Keys));
    assert!(MENU_ITEMS.iter().any(|i| i.action == MenuAction::About));
    assert!(MENU_ITEMS.iter().any(|i| i.action == MenuAction::ThemeToggle));
    assert_eq!(passport_core::keymap::ABOUT.len(), 9);

    let mut sh = shell_with_apps();
    let out = sh.apply_command(parse_line("about").unwrap());
    assert_eq!(sh.overlay(), Overlay::About);
    assert!(out.reply.contains("about"), "{}", out.reply);
}

fn open_menu(sh: &mut Shell) {
    sh.apply_command(parse_line("menu").unwrap());
    assert_eq!(sh.overlay(), Overlay::System);
}

fn select_menu_action(sh: &mut Shell, want: MenuAction) {
    let idx = MENU_ITEMS
        .iter()
        .position(|i| i.action == want)
        .expect("menu item exists");
    while sh.menu_selected() != idx {
        let out = sh.handle_event(ButtonEvent::Click(Key::Down));
        assert_eq!(out.side, SideEffect::None, "move must not fire side effects");
    }
}

#[test]
fn system_menu_ok_emits_same_side_effect_as_console() {
    // Brightness + via button OK vs `brightness N` on a shell with the same value.
    let mut buttons = shell_with_apps();
    open_menu(&mut buttons);
    let start = buttons.status.brightness;
    select_menu_action(&mut buttons, MenuAction::BrightnessInc);
    let from_btn = buttons.handle_event(ButtonEvent::Click(Key::Ok));
    let expected_bl = (start + 10).min(100);
    assert_eq!(from_btn.side, SideEffect::SetBrightness(expected_bl));

    let mut console = shell_with_apps();
    console.status.brightness = start;
    let cmd = parse_line(&format!("brightness {expected_bl}")).unwrap();
    let from_console = console.apply_command(cmd);
    assert_eq!(
        from_btn.side, from_console.side,
        "menu OK brightness must match console brightness command"
    );

    // Wi-Fi scan via millivolt-synthesized OK (ADC path) vs `radio wifi`.
    let mut buttons = shell_with_apps();
    open_menu(&mut buttons);
    select_menu_action(&mut buttons, MenuAction::RadioWifi);
    let from_btn = buttons.synth_click(Key::Ok);
    assert_eq!(from_btn.side, SideEffect::WifiScan);
    let from_console = shell_with_apps().apply_command(parse_line("radio wifi").unwrap());
    assert_eq!(from_btn.side, from_console.side);

    // Sleep light via handle_event OK vs `sleep light`.
    let mut buttons = shell_with_apps();
    open_menu(&mut buttons);
    select_menu_action(&mut buttons, MenuAction::SleepLight);
    let from_btn = buttons.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(from_btn.side, SideEffect::SleepLight);
    let from_console = shell_with_apps().apply_command(parse_line("sleep light").unwrap());
    assert_eq!(from_btn.side, from_console.side);
    assert_eq!(buttons.overlay(), Overlay::None);
}

#[test]
fn paint_plan_first_frame_is_full_not_whole_panel_hint() {
    let mut sh = shell_with_apps();
    sh.enter_home();
    let plan = PaintPlan::diff(None, FrameSig::capture(&sh));
    assert!(plan.wipe_content);
    assert_eq!(plan.wipe_rows(), LCD_H - STATUS_BAR_H);
    assert_ne!(plan.wipe_rows(), LCD_H, "content wipe must leave the status bar");
    assert!(plan.desktop);
    assert!(plan.status);
    assert!(!plan.pulse);
}

#[test]
fn idle_telemetry_is_off_so_loop_does_not_spam() {
    assert_eq!(IDLE_TELEMETRY_PERIOD_TICKS, 0);
    assert!(!idle_telemetry_due(0));
    assert!(!idle_telemetry_due(1));
    assert!(!idle_telemetry_due(25));
    assert!(!idle_telemetry_due(50));
    assert!(!idle_telemetry_due(250));
}

#[test]
fn battery_poll_is_seconds_not_subsecond() {
    // 250 ticks × 20 ms = 5 s. Tick 0 must not fire (loop starts at 1).
    assert!(!battery_poll_due(0));
    assert!(!battery_poll_due(25));
    assert!(!battery_poll_due(50));
    assert!(battery_poll_due(250));
    assert!(battery_poll_due(500));
}

#[test]
fn shell_boot_holds_no_audio_or_radio() {
    let sh = Shell::new();
    assert!(sh.exclusive.is_free(), "DMA/radio must not be acquired at idle boot");
}

#[test]
fn ntag213_ndef_uri_roundtrip_and_mcu_has_no_bus() {
    use passport_core::nfc::{decode_uri_tlv, encode_uri_tlv, mcu_read, mcu_write, DEFAULT_URI, NTAG213};
    use passport_core::ApiError;

    assert_eq!(NTAG213.name, "NTAG213");
    assert_eq!(NTAG213.user_bytes, 144);
    assert!(!NTAG213.mcu_wired);
    assert_eq!(mcu_read(4, &mut [0; 4]), Err(ApiError::NoBus));
    assert_eq!(mcu_write(4, &[1, 2, 3, 4]), Err(ApiError::NoBus));

    let mut buf = [0u8; 64];
    let n = encode_uri_tlv(DEFAULT_URI, &mut buf).expect("ndef fits NTAG213");
    assert!(n > 8);
    assert!(n <= usize::from(NTAG213.user_bytes));
    let back = decode_uri_tlv(&buf[..n]).expect("tlv parses");
    assert_eq!(back.as_str(), DEFAULT_URI);

    let n = encode_uri_tlv("http://www.example.com/x", &mut buf).unwrap();
    assert_eq!(decode_uri_tlv(&buf[..n]).unwrap().as_str(), "http://www.example.com/x");
}

#[test]
fn console_nfc_and_launcher_list_tap_app() {
    let mut sh = shell_with_apps();
    sh.register_app(AppId(3), "nfc").unwrap();
    sh.enter_home();
    let names = sh.launcher_names();
    assert!(
        names.iter().any(|n| *n == "nfc"),
        "launcher must list nfc, got {names:?}"
    );
    let out = sh.apply_command(parse_line("nfc").unwrap());
    assert!(out.reply.contains("NTAG213"), "{}", out.reply);
    assert!(out.reply.contains("mcu=none"), "{}", out.reply);
    let out = sh.apply_command(parse_line("activate nfc").unwrap());
    assert_eq!(sh.focused_app_name(), Some("nfc"));
    assert_eq!(sh.overlay(), Overlay::None);
    let _ = out;
}

#[test]
fn paint_plan_launcher_move_only_two_cards() {
    let mut sh = shell_with_apps();
    sh.enter_home();
    let prev = FrameSig::capture(&sh);
    sh.handle_event(ButtonEvent::Click(Key::Down));
    let plan = PaintPlan::diff(Some(prev), FrameSig::capture(&sh));
    assert!(!plan.wipe_content, "moving focus must not wipe the panel");
    assert_eq!(plan.wipe_rows(), 0);
    assert!(!plan.desktop);
    assert!(!plan.pulse);
    assert_eq!(plan.launcher_cards, Some((0, 1)));
}

#[test]
fn theme_toggle_from_menu_and_console() {
    let mut sh = shell_with_apps();
    assert_eq!(sh.theme(), Theme::Dark);
    open_menu(&mut sh);
    select_menu_action(&mut sh, MenuAction::ThemeToggle);
    let out = sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(out.side, SideEffect::None);
    assert_eq!(sh.theme(), Theme::Light);
    let out = sh.apply_command(parse_line("theme dark").unwrap());
    assert_eq!(sh.theme(), Theme::Dark);
    assert!(out.reply.contains("dark"), "{}", out.reply);
    let prev = FrameSig::capture(&sh);
    sh.apply_command(parse_line("theme light").unwrap());
    let plan = PaintPlan::diff(Some(prev), FrameSig::capture(&sh));
    assert!(plan.wipe_content, "theme change must repaint the page");
    assert!(plan.status);
}

#[test]
fn paint_plan_overlay_change_is_content_wipe_only() {
    let mut sh = shell_with_apps();
    sh.enter_home();
    let prev = FrameSig::capture(&sh);
    sh.handle_event(ButtonEvent::Click(Key::Ok));
    let plan = PaintPlan::diff(Some(prev), FrameSig::capture(&sh));
    assert!(plan.wipe_content);
    assert!(plan.pulse);
    assert!(!plan.desktop);
}

fn shell_with_flap() -> Shell {
    let mut sh = shell_with_apps();
    sh.register_app(FLAP_APP_ID, "flap").unwrap();
    sh
}

fn activate_flap(sh: &mut Shell) {
    sh.enter_home();
    sh.apply_command(parse_line("activate flap").unwrap());
}

#[test]
fn paint_plan_idle_flap_does_not_repaint_playfield() {
    let mut sh = shell_with_flap();
    activate_flap(&mut sh);
    let prev = FrameSig::capture(&sh);
    let plan = PaintPlan::diff(Some(prev), FrameSig::capture(&sh));
    assert!(!plan.wipe_content);
    assert!(!plan.game);
    assert!(!plan.game_live);
    assert!(plan.is_nop() || plan.status);
}

#[test]
fn paint_plan_live_tick_is_sprites_not_wipe() {
    let mut sh = shell_with_flap();
    activate_flap(&mut sh);
    let prev = FrameSig::capture(&sh);
    sh.request_live();
    let plan = PaintPlan::diff(Some(prev), FrameSig::capture(&sh));
    assert!(!plan.wipe_content, "live anim must not wipe the ST7789");
    assert!(!plan.game);
    assert!(plan.game_live);
    assert_eq!(plan.wipe_rows(), 0);
}

#[test]
fn paint_plan_game_scene_repaints_tile_without_panel_wipe() {
    let mut sh = shell_with_flap();
    activate_flap(&mut sh);
    let prev = FrameSig::capture(&sh);
    sh.request_full();
    let plan = PaintPlan::diff(Some(prev), FrameSig::capture(&sh));
    assert!(plan.game);
    assert!(!plan.game_live);
    assert!(!plan.wipe_content);
    assert_eq!(plan.wipe_rows(), 0);
}

#[test]
fn paint_plan_opening_flap_does_not_double_wipe() {
    let mut sh = shell_with_flap();
    sh.enter_home();
    let prev = FrameSig::capture(&sh);
    sh.apply_command(parse_line("activate flap").unwrap());
    let plan = PaintPlan::diff(Some(prev), FrameSig::capture(&sh));
    assert_eq!(sh.overlay(), Overlay::None);
    assert!(plan.game);
    assert!(!plan.wipe_content, "flap fills the tile; compositor must not also wipe");
    assert!(!plan.desktop);
}

#[test]
fn workspace_press_ok_reaches_flap() {
    let mut sh = shell_with_flap();
    activate_flap(&mut sh);
    assert_eq!(sh.focused_app_name(), Some("flap"));
    let out = sh.handle_event(ButtonEvent::Press(Key::Ok));
    assert!(
        out.lifecycle.iter().any(|n| matches!(
            n,
            AppLifecycle::Input(id, ButtonEvent::Press(Key::Ok)) if *id == FLAP_APP_ID
        )),
        "Press(Ok) must reach flap, got {:?}",
        out.lifecycle
    );
}

#[test]
fn rapid_ok_taps_each_reach_focused_flap() {
    let mut sh = shell_with_flap();
    activate_flap(&mut sh);
    let mut presses = 0u32;
    for cycle in 0..2 {
        for _ in 0..3 {
            let out = sh.tick_mv(TYPICAL_OK_MV, 20);
            presses += out
                .lifecycle
                .iter()
                .filter(|n| {
                    matches!(
                        n,
                        AppLifecycle::Input(id, ButtonEvent::Press(Key::Ok)) if *id == FLAP_APP_ID
                    )
                })
                .count() as u32;
        }
        let _ = sh.tick_mv(TYPICAL_RELEASED_MV, 20);
        let _ = cycle;
    }
    assert_eq!(presses, 2, "mashing OK must deliver two Press events to flap");
}

#[test]
fn game_up_down_does_not_steal_focus_or_workspace() {
    for (id, name) in [
        (FLAP_APP_ID, "flap"),
        (STACK_APP_ID, "stack"),
        (BRICK_APP_ID, "brick"),
    ] {
        let mut sh = shell_with_apps();
        sh.register_app(id, name).unwrap();
        sh.enter_home();
        sh.apply_command(parse_line("activate pulse").unwrap());
        sh.apply_command(parse_line(&format!("activate {name}")).unwrap());
        assert_eq!(sh.tile_count(), 2, "{name}");
        assert_eq!(sh.focused_app_name(), Some(name));
        let ws = sh.current_workspace();

        let out = sh.handle_event(ButtonEvent::Click(Key::Up));
        assert_eq!(sh.focused_app_name(), Some(name), "{name} click up stole focus");
        assert_eq!(sh.current_workspace(), ws);
        assert!(
            out.lifecycle.iter().any(|n| matches!(
                n, AppLifecycle::Input(i, ButtonEvent::Click(Key::Up)) if *i == id
            )),
            "{name} click must reach the game, got {:?}",
            out.lifecycle
        );

        let _ = sh.synth_click(Key::Down);
        assert_eq!(
            sh.focused_app_name(),
            Some(name),
            "{name} release-click must not hop to pulse"
        );

        let _ = sh.handle_event(ButtonEvent::LongPress(Key::Down));
        assert_eq!(
            sh.current_workspace(),
            ws,
            "{name} hold DOWN must not switch desk"
        );
        assert_eq!(sh.focused_app_name(), Some(name));

        sh.handle_event(ButtonEvent::LongPress(Key::Ok));
        assert_eq!(sh.overlay(), Overlay::Launcher, "{name} long OK is still home");
    }
}

#[test]
fn two_utility_apps_still_focus_with_up_down_click() {
    let mut sh = shell_with_apps();
    sh.start_on_current(PULSE);
    sh.start_on_current(METER);
    assert_eq!(sh.focused_app_name(), Some("meter"));
    sh.handle_event(ButtonEvent::Click(Key::Up));
    assert_eq!(sh.focused_app_name(), Some("pulse"));
    sh.handle_event(ButtonEvent::Click(Key::Down));
    assert_eq!(sh.focused_app_name(), Some("meter"));
}
