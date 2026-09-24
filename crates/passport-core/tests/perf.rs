//! Micro-benchmarks for the hot paths. Run with:
//!   cargo test --release -p passport-core --test perf -- --ignored --nocapture
//! No assertions — prints timings for RSI comparisons only.

use passport_core::pitch::{PITCH_N, PITCH_RATE, PitchBuf};
use std::time::Instant;

fn sine_pcm(hz: u32, samples: usize) -> Vec<u8> {
    // Stereo 16-bit LE: left carries the signal, right is silence.
    let mut out = Vec::with_capacity(samples * 4);
    for i in 0..samples {
        let t = i as f32 / PITCH_RATE as f32;
        let s = (t * hz as f32 * std::f32::consts::TAU).sin();
        let v = (s * 8000.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(&0i16.to_le_bytes());
    }
    out
}

#[test]
#[ignore]
fn pitch_amdf_cost() {
    let pcm = sine_pcm(440, 1024);
    let mut buf = PitchBuf::new();
    buf.push_pcm16_le_auto(&pcm);
    // warm
    let _ = buf.hz();
    let t = Instant::now();
    let mut acc = 0u32;
    for _ in 0..200 {
        acc += buf.hz().unwrap_or(0) as u32;
    }
    let per = t.elapsed() / 200;
    println!("amdf_hz(440Hz sine) ~{per:?}/call  (acc={acc})");

    let mut sh = passport_core::shell::Shell::new();
    sh.mic_begin();
    let t = Instant::now();
    for _ in 0..400 {
        sh.feed_pcm(&pcm);
    }
    println!("shell.feed_pcm(1KB stereo) ~{:?}/call", t.elapsed() / 400);
    let _ = PITCH_N;
}
