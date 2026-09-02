//! Host tests for the status-bar wall clock.

use passport_core::api::MemoryStore;
use passport_core::clock::{parse_hm, read_tod, write_tod, Clock};
use passport_core::console::parse_line;
use passport_core::paint::{FrameSig, PaintPlan};
use passport_core::shell::Shell;
use passport_core::Command;

#[test]
fn parse_and_format_roundtrip() {
    assert_eq!(parse_hm("14:32"), Ok((14, 32)));
    assert_eq!(parse_hm("9:05"), Ok((9, 5)));
    assert_eq!(parse_hm("00:00"), Ok((0, 0)));
    assert_eq!(parse_hm("23:59"), Ok((23, 59)));
    assert!(parse_hm("24:00").is_err());
    assert!(parse_hm("12:60").is_err());
    assert!(parse_hm("noon").is_err());
    let mut c = Clock::new();
    assert_eq!(c.format_hm().as_str(), "--:--");
    c.set_hm(9, 5).unwrap();
    assert_eq!(c.format_hm().as_str(), "09:05");
}

#[test]
fn minute_tick_and_midnight_wrap() {
    let mut c = Clock::new();
    assert!(!c.add_secs(60), "unset clock must not tick");
    c.set_hm(23, 59).unwrap();
    assert!(!c.add_secs(59));
    assert_eq!(c.hour_minute(), Some((23, 59)));
    assert!(c.add_secs(1));
    assert_eq!(c.hour_minute(), Some((0, 0)));
}

#[test]
fn store_roundtrip_minutes() {
    let mut store = MemoryStore::new();
    assert_eq!(read_tod(&store), None);
    write_tod(&mut store, 14 * 60 + 32);
    assert_eq!(read_tod(&store), Some(14 * 60 + 32));
}

#[test]
fn console_time_sets_status_and_dirties_bar() {
    let mut sh = Shell::new();
    assert_eq!(parse_line("time").unwrap(), Command::Time(None));
    assert_eq!(parse_line("time 14:32").unwrap(), Command::Time(Some((14, 32))));
    assert!(parse_line("time 25:00").is_err());
    let out = sh.apply_command(parse_line("time 14:32").unwrap());
    assert_eq!(out.side, passport_core::shell::SideEffect::ClockSet);
    assert!(out.reply.contains("14:32"), "{}", out.reply);
    assert_eq!(sh.status.clock.format_hm().as_str(), "14:32");
    let prev = FrameSig::capture(&sh);
    assert!(sh.advance_clock(59) == false);
    let plan = PaintPlan::diff(Some(prev), FrameSig::capture(&sh));
    assert!(!plan.status);
    let prev = FrameSig::capture(&sh);
    assert!(sh.advance_clock(1));
    let plan = PaintPlan::diff(Some(prev), FrameSig::capture(&sh));
    assert!(plan.status);
    assert!(!plan.wipe_content);
    assert_eq!(sh.status.clock.format_hm().as_str(), "14:33");
}
