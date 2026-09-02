//! 3-key English IME: UP/DOWN walk a visible grid, OK inserts or acts.
//!
//! Shift is sticky (Aa). `x` cancels, `go` submits. Designed for a 240 px
//! panel with a 6 px glyph cell — one character per key.

use heapless::String;

/// Max password / text length. Matches WPA PSK (63) plus a spare.
pub const IME_BUF: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImeKey {
    Char(char),
    Shift,
    Del,
    Space,
    Esc,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImeAction {
    None,
    Edit,
    Done,
    Cancel,
}

const fn c(ch: char) -> ImeKey {
    ImeKey::Char(ch)
}

pub const IME_ROW0: &[ImeKey] = &[
    c('q'),
    c('w'),
    c('e'),
    c('r'),
    c('t'),
    c('y'),
    c('u'),
    c('i'),
    c('o'),
    c('p'),
];
pub const IME_ROW1: &[ImeKey] = &[
    c('a'),
    c('s'),
    c('d'),
    c('f'),
    c('g'),
    c('h'),
    c('j'),
    c('k'),
    c('l'),
];
pub const IME_ROW2: &[ImeKey] = &[
    c('z'),
    c('x'),
    c('c'),
    c('v'),
    c('b'),
    c('n'),
    c('m'),
];
pub const IME_ROW3: &[ImeKey] = &[
    c('1'),
    c('2'),
    c('3'),
    c('4'),
    c('5'),
    c('6'),
    c('7'),
    c('8'),
    c('9'),
    c('0'),
];
pub const IME_ROW4: &[ImeKey] = &[
    c('-'),
    c('_'),
    c('.'),
    c('@'),
    c('/'),
    c('!'),
    c('?'),
    c(','),
];
pub const IME_ROW5: &[ImeKey] = &[
    ImeKey::Shift,
    ImeKey::Del,
    ImeKey::Space,
    ImeKey::Esc,
    ImeKey::Done,
];

pub const IME_ROWS: &[&[ImeKey]] = &[
    IME_ROW0, IME_ROW1, IME_ROW2, IME_ROW3, IME_ROW4, IME_ROW5,
];

pub const IME_KEY_COUNT: usize = IME_ROW0.len()
    + IME_ROW1.len()
    + IME_ROW2.len()
    + IME_ROW3.len()
    + IME_ROW4.len()
    + IME_ROW5.len();

pub fn key_at(idx: usize) -> ImeKey {
    let mut n = idx;
    for row in IME_ROWS {
        if n < row.len() {
            return row[n];
        }
        n -= row.len();
    }
    IME_ROW0[0]
}

impl ImeKey {
    pub fn write_label(self, shift: bool, out: &mut String<4>) {
        out.clear();
        match self {
            ImeKey::Char(ch) => {
                let g = if shift && ch.is_ascii_alphabetic() {
                    ch.to_ascii_uppercase()
                } else {
                    ch
                };
                let _ = out.push(g);
            }
            ImeKey::Shift => {
                let _ = out.push_str("Aa");
            }
            ImeKey::Del => {
                let _ = out.push_str("<");
            }
            ImeKey::Space => {
                let _ = out.push_str("_");
            }
            ImeKey::Esc => {
                let _ = out.push_str("x");
            }
            ImeKey::Done => {
                let _ = out.push_str("go");
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Ime {
    buf: String<IME_BUF>,
    cursor: usize,
    shift: bool,
}

impl Default for Ime {
    fn default() -> Self {
        Self::new()
    }
}

impl Ime {
    pub const fn new() -> Self {
        Self {
            buf: String::new(),
            cursor: 0,
            shift: false,
        }
    }

    pub fn reset(&mut self) {
        self.buf.clear();
        self.cursor = 0;
        self.shift = false;
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn shift(&self) -> bool {
        self.shift
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn buffer(&self) -> &str {
        self.buf.as_str()
    }

    pub fn current(&self) -> ImeKey {
        key_at(self.cursor)
    }

    pub fn move_sel(&mut self, delta: i16) {
        let n = IME_KEY_COUNT as i16;
        let mut s = self.cursor as i16 + delta;
        s = ((s % n) + n) % n;
        self.cursor = s as usize;
    }

    pub fn click(&mut self) -> ImeAction {
        match self.current() {
            ImeKey::Char(ch) => {
                let g = if self.shift && ch.is_ascii_alphabetic() {
                    ch.to_ascii_uppercase()
                } else {
                    ch
                };
                if self.buf.push(g).is_ok() {
                    ImeAction::Edit
                } else {
                    ImeAction::None
                }
            }
            ImeKey::Shift => {
                self.shift = !self.shift;
                ImeAction::Edit
            }
            ImeKey::Del => {
                let _ = self.buf.pop();
                ImeAction::Edit
            }
            ImeKey::Space => {
                let _ = self.buf.push(' ');
                ImeAction::Edit
            }
            ImeKey::Esc => ImeAction::Cancel,
            ImeKey::Done => ImeAction::Done,
        }
    }

    /// Masked field: bullets, last character shown so the typist can confirm.
    pub fn masked(&self) -> String<IME_BUF> {
        let mut s = String::new();
        let b = self.buf.as_str();
        let n = b.len();
        for (i, ch) in b.chars().enumerate() {
            if i + 1 == n {
                let _ = s.push(ch);
            } else {
                let _ = s.push('*');
            }
        }
        s
    }
}
