//! System mic API: `Shell::tick_mic` + `AppLifecycle::Mic`, not a Boo-only I2S path.

use passport_core::IDLE_STANDBY_MS;
use passport_core::app::{AppId, AppLifecycle};
use passport_core::board::TYPICAL_RELEASED_MV;
use passport_core::boo::BOO_APP_ID;
use passport_core::console::parse_line;
use passport_core::es8311::{self, adc_clocks_on};
use passport_core::mic::{MicEvent, ROAR_LEVEL, pcm16_le_level, pcm16_le_peak};
use passport_core::pitch::{PITCH_N, PITCH_RATE};
use passport_core::shell::{Overlay, Shell};

fn shell_boo() -> Shell {
    let mut sh = Shell::new();
    sh.register_app(BOO_APP_ID, "boo").unwrap();
    sh.set_wants_mic(BOO_APP_ID, true);
    sh.enter_home();
    sh.apply_command(parse_line("activate boo").unwrap());
    sh
}

#[test]
fn es8311_init_turns_adc_clocks_on() {
    let clk = es8311::last_write(0x01).expect("clock manager");
    assert!(
        adc_clocks_on(clk),
        "0x01={clk:#04x} must set CLKADC_ON and ANACLKADC_ON (0x30 is MCLK+BCLK only)"
    );
    assert_eq!(clk, es8311::CLK_ALL_ON);
    assert_eq!(es8311::last_write(0x00), Some(es8311::CSM_ON));
    let mic = es8311::last_write(0x14).expect("mic pga");
    assert_eq!(mic & 0x40, 0, "analog mic, not DMIC");
    assert_eq!(mic, es8311::MIC_ANALOG_PGA30);
    assert_eq!(es8311::last_write(0x44), Some(es8311::GPIO_NO_DAC_REF));
    assert_eq!(es8311::last_write(0x02), Some(0x20), "pre_div=2 for 256×fs");
    assert_eq!(es8311::last_write(0x07), Some(0x00), "lrck_h=0 not 0x01");
    assert!(
        !passport_core::board::I2S_RX_LOOPBACK_TX,
        "loopback stalled RX; retest pcm>0 before flipping"
    );
    assert!(
        !adc_clocks_on(0x30),
        "the old truncated open poke must not pass"
    );
    let start_clk = es8311::START
        .iter()
        .rev()
        .find(|(r, _)| *r == 0x01)
        .map(|(_, v)| *v)
        .expect("START clock manager");
    assert!(
        adc_clocks_on(start_clk),
        "START 0x01={start_clk:#04x} after I2S clocks"
    );
}

#[test]
fn pcm16_and_console_mic_are_system_entry_points() {
    assert_eq!(pcm16_le_level(&[0, 0, 0, 0]), 0);
    assert_eq!(pcm16_le_peak(&[0, 0, 0, 0]), 0);
    assert_eq!(
        pcm16_le_peak(&i16::MAX.to_le_bytes()),
        i16::MAX.unsigned_abs()
    );
    let mut sh = Shell::new();
    let out = sh.apply_command(parse_line("mic").unwrap());
    assert!(out.reply.starts_with("mic "), "{}", out.reply);
    assert!(out.reply.contains("roar="), "{}", out.reply);
    assert!(out.reply.contains("pcm="), "{}", out.reply);
    sh.note_mic_peak(1234);
    let out = sh.apply_command(parse_line("mic").unwrap());
    assert!(out.reply.contains("pcm=1234"), "{}", out.reply);
    assert!(out.reply.contains("hz="), "{}", out.reply);
}

#[test]
fn feed_pcm_reaches_tune_listen() {
    let mut sh = Shell::new();
    let mut pcm = Vec::new();
    for i in 0..PITCH_N {
        let t = i as f32 / PITCH_RATE as f32;
        let s = ((core::f32::consts::TAU * 440.0 * t).sin() * 18000.0) as i16;
        pcm.extend_from_slice(&s.to_le_bytes());
        pcm.extend_from_slice(&0i16.to_le_bytes());
    }
    sh.feed_pcm(&pcm);
    let mut tmp = [0i16; PITCH_N];
    let n = sh.pitch_buf().copy_linear(&mut tmp);
    let mut w = passport_core::TuneWorld::new();
    w.cycle_instrument(); // ukulele
    w.nudge_string(passport_core::input::ButtonEvent::Click(
        passport_core::board::Key::Down,
    ));
    w.nudge_string(passport_core::input::ButtonEvent::Click(
        passport_core::board::Key::Down,
    ));
    w.nudge_string(passport_core::input::ButtonEvent::Click(
        passport_core::board::Key::Down,
    ));
    assert_eq!(w.target().name, "A4");
    w.listen(&tmp[..n]);
    assert!(
        w.in_tune() || w.cents().unsigned_abs() <= 20,
        "A4 via feed_pcm cents {}",
        w.cents()
    );
}

#[test]
fn tick_mic_reaches_opted_in_desk_app_not_launcher() {
    let mut sh = shell_boo();
    assert_eq!(sh.overlay(), Overlay::None);
    assert!(sh.mic_listen());
    sh.mic_begin();
    let out = sh.tick_mic(ROAR_LEVEL);
    assert!(
        out.lifecycle
            .iter()
            .any(|n| matches!(n, AppLifecycle::Mic(id, MicEvent::Loud) if *id == BOO_APP_ID)),
        "desk boo must get Loud, got {:?}",
        out.lifecycle
    );
    assert_eq!(sh.overlay(), Overlay::None);
    assert_eq!(sh.focused_app_name(), Some("boo"));

    sh.apply_command(parse_line("launcher").unwrap());
    assert!(!sh.mic_listen());
    sh.mic_begin();
    let out = sh.tick_mic(ROAR_LEVEL);
    assert!(
        !out.lifecycle
            .iter()
            .any(|n| matches!(n, AppLifecycle::Mic(_, _))),
        "launcher must not steal mic, got {:?}",
        out.lifecycle
    );
}

#[test]
fn tick_mic_without_opt_in_is_silent() {
    let mut sh = Shell::new();
    sh.register_app(AppId(1), "pulse").unwrap();
    sh.enter_home();
    sh.apply_command(parse_line("activate pulse").unwrap());
    assert!(!sh.mic_listen());
    let out = sh.tick_mic(200);
    assert!(out.lifecycle.is_empty(), "{:?}", out.lifecycle);
}

#[test]
fn holding_a_yell_is_one_event() {
    let mut sh = shell_boo();
    sh.mic_begin();
    let a = sh.tick_mic(80);
    let b = sh.tick_mic(90);
    assert!(
        a.lifecycle
            .iter()
            .any(|n| matches!(n, AppLifecycle::Mic(_, MicEvent::Loud)))
    );
    assert!(
        !b.lifecycle
            .iter()
            .any(|n| matches!(n, AppLifecycle::Mic(_, _))),
        "second sample at the same yell must not re-fire, got {:?}",
        b.lifecycle
    );
}

#[test]
fn mic_listen_inhibits_idle_standby() {
    let mut sh = shell_boo();
    assert!(sh.mic_listen());
    let out = sh.tick_mv(TYPICAL_RELEASED_MV, IDLE_STANDBY_MS);
    assert!(!sh.is_standby(), "playing a mic app must not blank");
    assert_ne!(out.side, passport_core::SideEffect::SetBrightness(0));
}
