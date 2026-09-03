//! Factory-slot used/free and About storage page. Drives shipped `Shell` + millivolts.

use passport_core::board::{
    FLASH_APP_SIZE, FLASH_SIZE, Key, KeyState, TYPICAL_DOWN_MV, TYPICAL_OK_MV, TYPICAL_RELEASED_MV,
    decode_millivolts,
};
use passport_core::console::parse_line;
use passport_core::input::ButtonEvent;
use passport_core::shell::{Overlay, Shell};
use passport_core::storage::{
    ABOUT_PAGE_COUNT, ABOUT_PAGE_PRODUCT, ABOUT_PAGE_STORAGE, factory_free, factory_image_len,
    format_size, used_bar_width,
};

fn shell_about() -> Shell {
    let mut sh = Shell::new();
    sh.register_app(passport_core::AppId(1), "pulse").unwrap();
    sh.apply_command(parse_line("about").unwrap());
    sh
}

fn read_slice(img: &[u8]) -> impl FnMut(u32, &mut [u8]) -> bool + '_ {
    move |off, buf| {
        let o = off as usize;
        if o.checked_add(buf.len()).is_none_or(|end| end > img.len()) {
            return false;
        }
        buf.copy_from_slice(&img[o..o + buf.len()]);
        true
    }
}

#[test]
fn factory_image_len_walks_e9_header() {
    let mut img = [0u8; 64];
    img[0] = 0xE9;
    img[1] = 1;
    img[23] = 0;
    // one empty segment at offset 24
    let len = factory_image_len(read_slice(&img)).expect("parse");
    assert_eq!(len, 48, "24 header + 8 seg + 1 checksum, pad to 16 → 48");

    img[23] = 1;
    let mut hashed = [0u8; 80];
    hashed[..64].copy_from_slice(&img);
    let len = factory_image_len(read_slice(&hashed)).expect("hash");
    assert_eq!(len, 80, "padded 48 + 32-byte SHA");
}

#[test]
fn factory_image_len_rejects_bad_magic() {
    let mut img = [0u8; 32];
    img[0] = 0x00;
    img[1] = 1;
    assert_eq!(factory_image_len(read_slice(&img)), None);
}

#[test]
fn format_size_and_free_use_shipped_factory_slot() {
    assert_eq!(format_size(0).as_str(), "0 B");
    assert_eq!(format_size(512).as_str(), "512 B");
    assert_eq!(format_size(729_280).as_str(), "712 KB");
    assert_eq!(format_size(FLASH_APP_SIZE).as_str(), "3 MB");
    assert_eq!(format_size(FLASH_SIZE).as_str(), "8 MB");
    let used = 729_280u32;
    assert_eq!(factory_free(used), FLASH_APP_SIZE - used);
    assert_eq!(used_bar_width(0, 100), 0);
    assert_eq!(used_bar_width(FLASH_APP_SIZE, 100), 100);
    let mid = used_bar_width(FLASH_APP_SIZE / 2, 100);
    assert!(mid >= 49 && mid <= 51, "mid={mid}");
}

#[test]
fn about_down_opens_storage_page_ok_returns_to_menu() {
    let mut sh = shell_about();
    assert_eq!(sh.overlay(), Overlay::About);
    assert_eq!(sh.about_page(), ABOUT_PAGE_PRODUCT);
    assert_eq!(ABOUT_PAGE_COUNT, 2);

    sh.set_factory_used(729_280);
    assert_eq!(sh.factory_used(), Some(729_280));
    assert_eq!(sh.factory_free(), Some(FLASH_APP_SIZE - 729_280));

    sh.handle_event(ButtonEvent::Click(Key::Down));
    assert_eq!(
        sh.overlay(),
        Overlay::About,
        "Down is the storage page, not leave"
    );
    assert_eq!(sh.about_page(), ABOUT_PAGE_STORAGE);

    sh.handle_event(ButtonEvent::Click(Key::Up));
    assert_eq!(sh.about_page(), ABOUT_PAGE_PRODUCT);
    sh.handle_event(ButtonEvent::Click(Key::Down));
    sh.handle_event(ButtonEvent::Click(Key::Ok));
    assert_eq!(sh.overlay(), Overlay::System);
    assert_eq!(sh.about_page(), ABOUT_PAGE_PRODUCT);
}

#[test]
fn millivolt_down_on_about_opens_storage_not_menu() {
    assert_eq!(
        decode_millivolts(TYPICAL_DOWN_MV),
        KeyState::Down(Key::Down)
    );
    assert_eq!(decode_millivolts(TYPICAL_OK_MV), KeyState::Down(Key::Ok));
    let mut sh = shell_about();
    sh.set_factory_used(100_000);
    let _ = sh.synth_click(Key::Down);
    assert_eq!(sh.overlay(), Overlay::About);
    assert_eq!(sh.about_page(), ABOUT_PAGE_STORAGE);
    assert_eq!(sh.factory_free(), Some(FLASH_APP_SIZE - 100_000));
    let _ = sh.tick_mv(TYPICAL_RELEASED_MV, 20);
    let _ = sh.synth_click(Key::Ok);
    assert_eq!(sh.overlay(), Overlay::System);
}
