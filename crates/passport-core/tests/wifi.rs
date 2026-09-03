//! Wi-Fi picker + 3-key English IME. Drives shipped `Ime` / `WifiUi` / `Shell`.

use passport_core::board::Key;
use passport_core::console::parse_line;
use passport_core::ime::{IME_KEY_COUNT, IME_ROWS, Ime, ImeAction, ImeKey, key_at};
use passport_core::input::ButtonEvent;
use passport_core::menu::{MENU_ITEMS, MenuAction};
use passport_core::paint::{FrameSig, PaintPlan};
use passport_core::shell::{Overlay, Shell, SideEffect};
use passport_core::wifi::{WIFI_VISIBLE, WifiNet, WifiPhase};

fn find_char(ch: char) -> usize {
    let want = ch.to_ascii_lowercase();
    (0..IME_KEY_COUNT)
        .find(|&i| matches!(key_at(i), ImeKey::Char(c) if c == want))
        .expect("char on IME grid")
}

fn find_key(k: ImeKey) -> usize {
    (0..IME_KEY_COUNT)
        .find(|&i| key_at(i) == k)
        .expect("key on IME grid")
}

fn ime_goto(ime: &mut Ime, idx: usize) {
    let n = IME_KEY_COUNT;
    let cur = ime.cursor();
    let forward = (idx + n - cur) % n;
    let back = (cur + n - idx) % n;
    if forward <= back {
        for _ in 0..forward {
            ime.move_sel(1);
        }
    } else {
        for _ in 0..back {
            ime.move_sel(-1);
        }
    }
    assert_eq!(ime.cursor(), idx);
}

fn ime_type(ime: &mut Ime, text: &str) {
    for ch in text.chars() {
        let want_shift = ch.is_ascii_uppercase();
        if ime.shift() != want_shift {
            ime_goto(ime, find_key(ImeKey::Shift));
            assert_eq!(ime.click(), ImeAction::Edit);
        }
        ime_goto(ime, find_char(ch));
        assert_eq!(ime.click(), ImeAction::Edit);
    }
}

fn open_wifi_menu(sh: &mut Shell) -> passport_core::shell::EventOutcome {
    sh.apply_command(parse_line("menu").unwrap());
    let idx = MENU_ITEMS
        .iter()
        .position(|i| i.action == MenuAction::RadioWifi)
        .unwrap();
    while sh.menu_selected() != idx {
        sh.handle_event(ButtonEvent::Click(Key::Down));
    }
    sh.handle_event(ButtonEvent::Click(Key::Ok))
}

fn sample_nets() -> heapless::Vec<WifiNet, 16> {
    let mut v = heapless::Vec::new();
    let _ = v.push(WifiNet::new("Cafe", true, -70).unwrap());
    let _ = v.push(WifiNet::new("Home", false, -40).unwrap());
    let _ = v.push(WifiNet::new("Home", false, -55).unwrap());
    let _ = v.push(WifiNet::new("Guest", false, -80).unwrap());
    v
}

fn shell_goto_ime_key(sh: &mut Shell, idx: usize) {
    let n = IME_KEY_COUNT;
    let cur = sh.wifi().ime().cursor();
    let forward = (idx + n - cur) % n;
    let back = (cur + n - idx) % n;
    if forward <= back {
        for _ in 0..forward {
            let out = sh.handle_event(ButtonEvent::Click(Key::Down));
            assert_eq!(out.side, SideEffect::None);
        }
    } else {
        for _ in 0..back {
            let out = sh.handle_event(ButtonEvent::Click(Key::Up));
            assert_eq!(out.side, SideEffect::None);
        }
    }
    assert_eq!(sh.wifi().ime().cursor(), idx);
}

fn shell_type(sh: &mut Shell, text: &str) {
    for ch in text.chars() {
        let want_shift = ch.is_ascii_uppercase();
        if sh.wifi().ime().shift() != want_shift {
            shell_goto_ime_key(sh, find_key(ImeKey::Shift));
            sh.handle_event(ButtonEvent::Click(Key::Ok));
        }
        shell_goto_ime_key(sh, find_char(ch));
        sh.handle_event(ButtonEvent::Click(Key::Ok));
    }
}

#[test]
fn ime_grid_is_qwerty_plus_actions() {
    assert_eq!(
        IME_ROWS.iter().map(|r| r.len()).sum::<usize>(),
        IME_KEY_COUNT
    );
    assert_eq!(key_at(0), ImeKey::Char('q'));
    assert_eq!(key_at(find_char('a')), ImeKey::Char('a'));
    assert_eq!(key_at(IME_KEY_COUNT - 1), ImeKey::Done);
}

#[test]
fn ime_types_shift_del_space_and_masks() {
    let mut ime = Ime::new();
    ime_type(&mut ime, "Hi");
    ime_goto(&mut ime, find_key(ImeKey::Space));
    assert_eq!(ime.click(), ImeAction::Edit);
    ime_type(&mut ime, "9");
    assert_eq!(ime.buffer(), "Hi 9");
    assert_eq!(ime.masked().as_str(), "***9");

    ime_goto(&mut ime, find_key(ImeKey::Del));
    assert_eq!(ime.click(), ImeAction::Edit);
    assert_eq!(ime.buffer(), "Hi ");
    assert_eq!(ime.masked().as_str(), "** ");

    ime_goto(&mut ime, find_key(ImeKey::Done));
    assert_eq!(ime.click(), ImeAction::Done);

    ime_goto(&mut ime, find_key(ImeKey::Esc));
    assert_eq!(ime.click(), ImeAction::Cancel);
}

#[test]
fn ime_wraps_up_from_first_key() {
    let mut ime = Ime::new();
    ime.move_sel(-1);
    assert_eq!(ime.cursor(), IME_KEY_COUNT - 1);
    ime.move_sel(1);
    assert_eq!(ime.cursor(), 0);
}

#[test]
fn menu_wifi_and_console_open_scan_overlay() {
    let mut buttons = Shell::new();
    let from_btn = open_wifi_menu(&mut buttons);
    assert_eq!(from_btn.side, SideEffect::WifiScan);
    assert_eq!(buttons.overlay(), Overlay::Wifi);
    assert_eq!(buttons.wifi().phase(), WifiPhase::Scan);

    let from_console = Shell::new().apply_command(parse_line("radio wifi").unwrap());
    assert_eq!(from_btn.side, from_console.side);
    assert!(
        from_console.reply.contains("radio=wifi"),
        "{}",
        from_console.reply
    );
}

#[test]
fn scan_dedups_ssid_keeps_strongest_and_sorts() {
    let mut sh = Shell::new();
    sh.apply_command(parse_line("radio wifi").unwrap());
    sh.apply_wifi_scan(&sample_nets());
    assert_eq!(sh.wifi().phase(), WifiPhase::List);
    let nets = sh.wifi().nets();
    assert_eq!(nets.len(), 3, "duplicate Home must collapse");
    assert_eq!(nets[0].ssid.as_str(), "Home");
    assert_eq!(nets[0].rssi, -40);
    assert!(!nets[0].open);
    assert_eq!(nets[1].ssid.as_str(), "Cafe");
    assert!(nets[1].open);
    assert_eq!(nets[2].ssid.as_str(), "Guest");
}

#[test]
fn open_net_skips_ime_and_connects() {
    let mut sh = Shell::new();
    sh.apply_command(parse_line("radio wifi").unwrap());
    sh.apply_wifi_scan(&sample_nets());
    // Home (locked) is strongest / index 0; Cafe is 1.
    sh.handle_event(ButtonEvent::Click(Key::Down));
    assert_eq!(sh.wifi().nets()[sh.wifi().selected()].ssid.as_str(), "Cafe");
    let out = sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(out.side, SideEffect::WifiConnect);
    assert_eq!(sh.wifi().phase(), WifiPhase::Connecting);
    assert_eq!(sh.wifi_connect_ssid(), "Cafe");
    assert!(sh.wifi_connect_open());
    assert_eq!(sh.wifi_connect_pass(), "");
}

#[test]
fn locked_net_opens_ime_then_connects_with_password() {
    let mut sh = Shell::new();
    sh.apply_command(parse_line("radio wifi").unwrap());
    sh.apply_wifi_scan(&sample_nets());
    assert_eq!(sh.wifi().nets()[0].ssid.as_str(), "Home");
    let out = sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(out.side, SideEffect::None);
    assert_eq!(sh.wifi().phase(), WifiPhase::Ime);

    shell_type(&mut sh, "Secret1");
    assert_eq!(sh.wifi().ime().buffer(), "Secret1");
    assert_eq!(sh.wifi().ime().masked().as_str(), "******1");

    shell_goto_ime_key(&mut sh, find_key(ImeKey::Done));
    let out = sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(out.side, SideEffect::WifiConnect);
    assert_eq!(sh.wifi().phase(), WifiPhase::Connecting);
    assert_eq!(sh.wifi_connect_ssid(), "Home");
    assert!(!sh.wifi_connect_open());
    assert_eq!(sh.wifi_connect_pass(), "Secret1");
}

#[test]
fn empty_password_on_locked_net_does_not_connect() {
    let mut sh = Shell::new();
    sh.apply_command(parse_line("radio wifi").unwrap());
    sh.apply_wifi_scan(&sample_nets());
    sh.handle_event(ButtonEvent::Click(Key::Ok));
    shell_goto_ime_key(&mut sh, find_key(ImeKey::Done));
    let out = sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(out.side, SideEffect::None);
    assert_eq!(sh.wifi().phase(), WifiPhase::Ime);
}

#[test]
fn ime_esc_returns_to_list() {
    let mut sh = Shell::new();
    sh.apply_command(parse_line("radio wifi").unwrap());
    sh.apply_wifi_scan(&sample_nets());
    sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(sh.wifi().phase(), WifiPhase::Ime);
    shell_goto_ime_key(&mut sh, find_key(ImeKey::Esc));
    let out = sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(out.side, SideEffect::None);
    assert_eq!(sh.wifi().phase(), WifiPhase::List);
}

#[test]
fn list_scan_row_rescans() {
    let mut sh = Shell::new();
    sh.apply_command(parse_line("radio wifi").unwrap());
    sh.apply_wifi_scan(&sample_nets());
    let last = sh.wifi().row_count() - 1;
    assert!(sh.wifi().row_is_scan(last));
    while sh.wifi().selected() != last {
        sh.handle_event(ButtonEvent::Click(Key::Down));
    }
    let out = sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(out.side, SideEffect::WifiScan);
    assert_eq!(sh.wifi().phase(), WifiPhase::Scan);
}

#[test]
fn connect_result_and_long_ok_home() {
    let mut sh = Shell::new();
    sh.apply_command(parse_line("radio wifi").unwrap());
    sh.apply_wifi_scan(&sample_nets());
    sh.handle_event(ButtonEvent::Click(Key::Down));
    sh.handle_event(ButtonEvent::Click(Key::Ok));
    sh.apply_wifi_result(true);
    assert_eq!(sh.wifi().phase(), WifiPhase::Result);
    assert!(sh.wifi().result_ok());
    assert_eq!(sh.wifi().connected_ssid(), "Cafe");
    sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(sh.wifi().phase(), WifiPhase::List);

    sh.apply_wifi_result(false);
    assert!(!sh.wifi().result_ok());

    let out = sh.handle_event(ButtonEvent::LongPress(Key::Ok));
    assert_eq!(out.side, SideEffect::None);
    assert_eq!(sh.overlay(), Overlay::Launcher);
}

#[test]
fn millivolt_ok_on_wifi_matches_handle_event() {
    let mut sh = Shell::new();
    let _ = open_wifi_menu(&mut sh);
    sh.apply_wifi_scan(&sample_nets());
    sh.handle_event(ButtonEvent::Click(Key::Down));
    let from_ev = {
        let mut a = Shell::new();
        a.apply_command(parse_line("radio wifi").unwrap());
        a.apply_wifi_scan(&sample_nets());
        a.handle_event(ButtonEvent::Click(Key::Down));
        a.handle_event(ButtonEvent::Click(Key::Ok))
    };
    let from_adc = sh.synth_click(Key::Ok);
    assert_eq!(from_ev.side, from_adc.side);
    assert_eq!(from_adc.side, SideEffect::WifiConnect);
}

#[test]
fn paint_plan_wifi_list_move_is_two_rows() {
    let mut sh = Shell::new();
    sh.apply_command(parse_line("radio wifi").unwrap());
    sh.apply_wifi_scan(&sample_nets());
    let prev = FrameSig::capture(&sh);
    sh.handle_event(ButtonEvent::Click(Key::Down));
    let plan = PaintPlan::diff(Some(prev), FrameSig::capture(&sh));
    assert!(!plan.wipe_content);
    assert_eq!(plan.wifi_rows, Some((0, 1)));
    assert!(!plan.wifi);
}

#[test]
fn paint_plan_wifi_phase_change_wipes_body() {
    let mut sh = Shell::new();
    sh.apply_command(parse_line("menu").unwrap());
    let prev = FrameSig::capture(&sh);
    open_wifi_menu(&mut sh);
    let plan = PaintPlan::diff(Some(prev), FrameSig::capture(&sh));
    assert!(plan.wipe_content);
    assert!(plan.wifi);
    assert_eq!(
        plan.wipe_rows(),
        passport_core::board::LCD_H - passport_core::compositor::STATUS_BAR_H
    );

    sh.apply_wifi_scan(&sample_nets());
    let prev = FrameSig::capture(&sh);
    sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(sh.wifi().phase(), WifiPhase::Ime);
    let plan = PaintPlan::diff(Some(prev), FrameSig::capture(&sh));
    assert!(plan.wipe_content);
    assert!(plan.wifi);
}

#[test]
fn paint_plan_ime_cursor_is_two_keys() {
    let mut sh = Shell::new();
    sh.apply_command(parse_line("radio wifi").unwrap());
    sh.apply_wifi_scan(&sample_nets());
    sh.handle_event(ButtonEvent::Click(Key::Ok));
    let prev = FrameSig::capture(&sh);
    sh.handle_event(ButtonEvent::Click(Key::Down));
    let plan = PaintPlan::diff(Some(prev), FrameSig::capture(&sh));
    assert!(!plan.wipe_content);
    assert!(!plan.wifi);
    assert_eq!(plan.ime_keys, Some((0, 1)));
    assert!(plan.ime);
    assert!(!plan.ime_field);
}

#[test]
fn paint_plan_ime_insert_is_field_only() {
    let mut sh = Shell::new();
    sh.apply_command(parse_line("radio wifi").unwrap());
    sh.apply_wifi_scan(&sample_nets());
    sh.handle_event(ButtonEvent::Click(Key::Ok));
    // cursor is on 'q'
    let prev = FrameSig::capture(&sh);
    sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(sh.wifi().ime().buffer(), "q");
    let plan = PaintPlan::diff(Some(prev), FrameSig::capture(&sh));
    assert!(!plan.wipe_content);
    assert!(plan.ime_field);
    assert!(plan.ime_keys.is_none());
}

#[test]
fn list_window_scrolls_around_selection() {
    let mut nets = heapless::Vec::<WifiNet, 16>::new();
    for i in 0..12u8 {
        let mut name = heapless::String::<32>::new();
        let _ = core::fmt::Write::write_fmt(&mut name, format_args!("n{i}"));
        let _ = nets.push(WifiNet {
            ssid: name,
            open: true,
            rssi: -30 - i as i8,
        });
    }
    let mut sh = Shell::new();
    sh.apply_command(parse_line("radio wifi").unwrap());
    sh.apply_wifi_scan(&nets);
    assert_eq!(sh.wifi().row_count(), 13);
    let (start, len) = sh.wifi().window(WIFI_VISIBLE);
    assert_eq!(start, 0);
    assert_eq!(len, WIFI_VISIBLE);
    while sh.wifi().selected() != 12 {
        sh.handle_event(ButtonEvent::Click(Key::Down));
    }
    let (start, len) = sh.wifi().window(WIFI_VISIBLE);
    assert_eq!(len, WIFI_VISIBLE);
    assert_eq!(start, 13 - WIFI_VISIBLE);
}
