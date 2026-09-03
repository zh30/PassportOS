//! Factory-slot used / free. There is no filesystem; the OS disk is the 3 MB app slot.

use core::fmt::Write;
use heapless::String;

use crate::board::{FLASH_APP_SIZE, FLASH_SIZE};

/// Product copy, then storage. UP/DOWN on About flips these.
pub const ABOUT_PAGE_COUNT: u8 = 2;
pub const ABOUT_PAGE_PRODUCT: u8 = 0;
pub const ABOUT_PAGE_STORAGE: u8 = 1;

const ESP_IMAGE_MAGIC: u8 = 0xE9;
const ESP_IMAGE_HEADER: u32 = 24;

/// Human size for the 6 px About font (`"712 KB"`, `"2.3 MB"`).
pub fn format_size(n: u32) -> String<12> {
    let mut s = String::new();
    if n >= 1024 * 1024 {
        let tenth = (u64::from(n) * 10) / (1024 * 1024);
        let whole = tenth / 10;
        let frac = tenth % 10;
        if frac == 0 {
            let _ = write!(s, "{whole} MB");
        } else {
            let _ = write!(s, "{whole}.{frac} MB");
        }
    } else if n >= 1024 {
        let _ = write!(s, "{} KB", n / 1024);
    } else {
        let _ = write!(s, "{n} B");
    }
    s
}

pub fn factory_free(used: u32) -> u32 {
    FLASH_APP_SIZE.saturating_sub(used)
}

pub fn used_bar_width(used: u32, max_w: u16) -> u16 {
    if max_w == 0 {
        return 0;
    }
    let used = used.min(FLASH_APP_SIZE);
    ((u64::from(used) * u64::from(max_w)) / u64::from(FLASH_APP_SIZE)) as u16
}

pub const fn flash_total() -> u32 {
    FLASH_SIZE
}

pub const fn factory_total() -> u32 {
    FLASH_APP_SIZE
}

/// Walk an ESP32-C3 app image: 24-byte 0xE9 header, segments, checksum, optional SHA-256.
///
/// `read(offset, buf)` copies from the start of the factory image (offset 0 = `FLASH_APP_OFFSET`).
pub fn factory_image_len(mut read: impl FnMut(u32, &mut [u8]) -> bool) -> Option<u32> {
    let mut hdr = [0u8; 24];
    if !read(0, &mut hdr) {
        return None;
    }
    if hdr[0] != ESP_IMAGE_MAGIC {
        return None;
    }
    let nseg = hdr[1];
    if nseg == 0 || nseg > 16 {
        return None;
    }
    let hash_appended = hdr[23] != 0;
    let mut off = ESP_IMAGE_HEADER;
    for _ in 0..nseg {
        let mut seg = [0u8; 8];
        if !read(off, &mut seg) {
            return None;
        }
        let data_len = u32::from_le_bytes([seg[4], seg[5], seg[6], seg[7]]);
        off = off.saturating_add(8).saturating_add(data_len);
        if off > FLASH_APP_SIZE {
            return None;
        }
    }
    off = off.saturating_add(1);
    off = (off + 15) & !15;
    if hash_appended {
        off = off.saturating_add(32);
    }
    if off == 0 || off > FLASH_APP_SIZE {
        return None;
    }
    Some(off)
}
