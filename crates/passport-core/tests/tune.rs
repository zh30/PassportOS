//! Host tests for the tuner. Pitch is AMDF on shipped PCM, not I2S.

use passport_core::api::{MeteredDraw, NullDraw};
use passport_core::board::{Key, LCD_H, LCD_W};
use passport_core::compositor::{LIVE_SPI_BUDGET, Rect, STATUS_BAR_H, rgb565_bytes};
use passport_core::flap::{Redraw, cadence_redraw};
use passport_core::input::ButtonEvent;
use passport_core::pitch::{PITCH_N, PITCH_RATE, PitchBuf, amdf_hz, cents, fold_octave};
use passport_core::theme::Palette;
use passport_core::tune::{
    Instrument, TuneWorld, WORLD_H, WORLD_W, is_tune_instrument_key, is_tune_string_key,
    live_budget_holds,
};

fn vp() -> Rect {
    Rect {
        x: 0,
        y: STATUS_BAR_H,
        w: LCD_W,
        h: LCD_H - STATUS_BAR_H,
    }
}

fn stereo_sine(hz: f32, frames: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(frames * 4);
    for i in 0..frames {
        let t = i as f32 / PITCH_RATE as f32;
        let s = (core::f32::consts::TAU * hz * t).sin() * 18000.0;
        let v = s as i16;
        out.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(&0i16.to_le_bytes());
    }
    out
}

fn spi_of(world: &TuneWorld, mode: Redraw) -> u32 {
    let mut n = NullDraw::new(vp());
    let mut m = MeteredDraw::new(&mut n);
    world.paint(&mut m, vp(), mode, Palette::DARK);
    m.spi_bytes()
}

#[test]
fn a4_sine_is_near_440() {
    let pcm = stereo_sine(440.0, PITCH_N);
    let mut buf = PitchBuf::new();
    buf.push_pcm16_le_left(&pcm);
    let hz = buf.hz_near(440).expect("tone");
    assert!((hz as i32 - 440).unsigned_abs() <= 8, "A4 AMDF got {hz}");
}

#[test]
fn guitar_e2_sine_is_near_82() {
    let pcm = stereo_sine(82.4, PITCH_N);
    let mut buf = PitchBuf::new();
    buf.push_pcm16_le_left(&pcm);
    let hz = buf.hz_near(82).expect("E2");
    assert!((hz as i32 - 82).unsigned_abs() <= 4, "E2 AMDF got {hz}");
}

#[test]
fn silence_is_none() {
    let mut buf = PitchBuf::new();
    buf.push_pcm16_le_left(&[0u8; PITCH_N * 4]);
    assert_eq!(buf.hz(), None);
    assert_eq!(amdf_hz(&[0; 128], PITCH_RATE, 70, 900), None);
}

#[test]
fn cents_of_a_semitone_is_about_100() {
    // 466.16 Hz is A#4, one semitone above 440.
    let c = cents(466, 440);
    assert!((c - 100).unsigned_abs() <= 8, "semitone cents got {c}");
    assert_eq!(cents(440, 440), 0);
}

#[test]
fn fold_octave_halves_double() {
    assert_eq!(fold_octave(880, 440), 440);
    assert_eq!(fold_octave(220, 440), 440);
}

#[test]
fn guitar_has_six_uke_and_violin_four() {
    assert_eq!(Instrument::Guitar.strings().len(), 6);
    assert_eq!(Instrument::Ukulele.strings().len(), 4);
    assert_eq!(Instrument::Violin.strings().len(), 4);
    assert_eq!(Instrument::Guitar.strings()[0].name, "E2");
    assert_eq!(Instrument::Ukulele.strings()[3].name, "A4");
    assert_eq!(Instrument::Violin.strings()[3].hz, 659);
}

#[test]
fn ok_cycles_instrument_up_down_picks_string() {
    let mut w = TuneWorld::new();
    assert_eq!(w.instrument, Instrument::Guitar);
    assert_eq!(w.target().name, "E2");
    w.nudge_string(ButtonEvent::Click(Key::Down));
    assert_eq!(w.target().name, "A2");
    w.nudge_string(ButtonEvent::Click(Key::Up));
    assert_eq!(w.target().name, "E2");
    w.cycle_instrument();
    assert_eq!(w.instrument, Instrument::Ukulele);
    assert_eq!(w.target().name, "G4");
    w.cycle_instrument();
    assert_eq!(w.instrument, Instrument::Violin);
    w.cycle_instrument();
    assert_eq!(w.instrument, Instrument::Guitar);
    assert!(is_tune_string_key(ButtonEvent::Click(Key::Up)));
    assert!(is_tune_instrument_key(ButtonEvent::Click(Key::Ok)));
    assert!(!is_tune_instrument_key(ButtonEvent::Press(Key::Ok)));
}

#[test]
fn listen_near_selected_string_scores_in_tune() {
    let pcm = stereo_sine(110.0, PITCH_N);
    let mut buf = PitchBuf::new();
    buf.push_pcm16_le_left(&pcm);
    let mut tmp = [0i16; PITCH_N];
    let n = buf.copy_linear(&mut tmp);
    let mut w = TuneWorld::new();
    w.nudge_string(ButtonEvent::Click(Key::Down)); // A2
    w.listen(&tmp[..n]);
    assert!(w.hz() > 0, "must hear A2");
    assert!(
        w.in_tune() || w.cents().unsigned_abs() <= 12,
        "A2 cents {}",
        w.cents()
    );
}

#[test]
fn live_needle_stays_in_spi_budget() {
    assert!(live_budget_holds());
    let mut w = TuneWorld::new();
    w.feed_hz(110);
    w.mark_painted();
    w.feed_hz(112);
    assert_eq!(w.redraw(), Redraw::Live);
    let live = w.live_spi_bytes();
    assert!(live <= LIVE_SPI_BUDGET, "live {live}");
    let painted = spi_of(&w, Redraw::Live);
    assert!(painted <= LIVE_SPI_BUDGET, "paint {painted}");
    let full = rgb565_bytes(WORLD_W as u16, WORLD_H as u16);
    assert!(full > LIVE_SPI_BUDGET);
    assert!(painted < full);
    assert_eq!(cadence_redraw(Redraw::Live, false), Redraw::None);
}

#[test]
fn string_change_is_full_redraw() {
    let mut w = TuneWorld::new();
    w.mark_painted();
    assert_eq!(w.redraw(), Redraw::Live);
    w.nudge_string(ButtonEvent::Click(Key::Down));
    assert_eq!(w.redraw(), Redraw::Full);
}
