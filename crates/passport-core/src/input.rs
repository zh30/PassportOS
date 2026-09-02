//! Click / long-press decoder. Firmware feeds ADC millivolts; console injects the same path.

use heapless::Vec;

use crate::board::{Key, KeyState, decode_millivolts};

/// Press / key-to-key settle. The ladder walks through the OK window, so a
/// press must stay put this long. Released (~3300 mV) cannot be a walk.
pub const DEBOUNCE_MS: u32 = 30;
/// End a hold on the first Released sample at the input cadence (5 ms).
/// Symmetric 30 ms settle swallowed rapid OK taps: the gap never stayed
/// Released long enough, so the decoder remained `Down(Ok)`.
pub const RELEASE_DEBOUNCE_MS: u32 = 5;
pub const LONG_PRESS_MS: u32 = 800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonEvent {
    Press(Key),
    Release(Key),
    Click(Key),
    LongPress(Key),
}

impl ButtonEvent {
    pub fn key(self) -> Key {
        match self {
            ButtonEvent::Press(k)
            | ButtonEvent::Release(k)
            | ButtonEvent::Click(k)
            | ButtonEvent::LongPress(k) => k,
        }
    }
}

/// Debounced press/click/long-press state machine over millivolt samples.
pub struct ButtonDecoder {
    current: KeyState,
    candidate: KeyState,
    stable_ms: u32,
    held_ms: u32,
    long_fired: bool,
}

impl Default for ButtonDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl ButtonDecoder {
    pub const fn new() -> Self {
        Self {
            current: KeyState::Released,
            candidate: KeyState::Released,
            stable_ms: 0,
            held_ms: 0,
            long_fired: false,
        }
    }

    pub fn current(&self) -> KeyState {
        self.current
    }

    /// Advance the decoder by `dt_ms` using a millivolt sample from ADC1_CH0.
    pub fn feed(&mut self, mv: u16, dt_ms: u32) -> Vec<ButtonEvent, 4> {
        let mut out = Vec::new();
        let raw = decode_millivolts(mv);
        if raw != self.candidate {
            self.candidate = raw;
            self.stable_ms = 0;
        }
        // This sample already covers `dt_ms` since the last reading.
        self.stable_ms = self.stable_ms.saturating_add(dt_ms);

        if self.candidate != self.current
            && self.stable_ms >= settle_ms(self.current, self.candidate)
        {
            self.emit_transition(&mut out);
        }

        if let KeyState::Down(k) = self.current {
            self.held_ms = self.held_ms.saturating_add(dt_ms);
            if !self.long_fired && self.held_ms >= LONG_PRESS_MS {
                self.long_fired = true;
                let _ = out.push(ButtonEvent::LongPress(k));
            }
        }
        out
    }

    fn emit_transition(&mut self, out: &mut Vec<ButtonEvent, 4>) {
        if let KeyState::Down(k) = self.current {
            let _ = out.push(ButtonEvent::Release(k));
            // Resistor ladder: UP/DOWN voltages pass through the OK window.
            // Click only when the pin actually returns to released — not when
            // the reading walks OK → DOWN → UP during a single press.
            if !self.long_fired && matches!(self.candidate, KeyState::Released) {
                let _ = out.push(ButtonEvent::Click(k));
            }
        }
        if let KeyState::Down(k) = self.candidate {
            let _ = out.push(ButtonEvent::Press(k));
            self.held_ms = 0;
            self.long_fired = false;
        } else {
            self.held_ms = 0;
            self.long_fired = false;
        }
        self.current = self.candidate;
        self.stable_ms = 0;
    }
}

fn settle_ms(from: KeyState, to: KeyState) -> u32 {
    match (from, to) {
        (KeyState::Down(_), KeyState::Released) => RELEASE_DEBOUNCE_MS,
        _ => DEBOUNCE_MS,
    }
}
