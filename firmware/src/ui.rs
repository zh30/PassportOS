//! Apple grouped-table chrome. Dark is the proven invert-on field; light page
//! fills are 8-row bands (a single 240×320 white RAMWR painted as 花屏).

use core::fmt::Write as _;

use passport_core::Theme;
use passport_core::board::{FLASH_KV_SIZE, FLASH_SIZE};
use passport_core::boot::BootAnim;
use passport_core::compositor::{Rect, STATUS_BAR_H};
use passport_core::ime::{IME_ROWS, ImeKey};
use passport_core::keymap::{ABOUT, CHEATSHEET};
use passport_core::menu::{MENU_ITEMS, MenuAction};
use passport_core::paint::{FrameSig, PaintPlan};
use passport_core::shell::{Overlay, Shell};
use passport_core::status::RadioMode;
use passport_core::storage::{ABOUT_PAGE_STORAGE, factory_total, format_size, used_bar_width};
use passport_core::theme::Palette;
use passport_core::wifi::{WIFI_VISIBLE, WifiPhase};
use passport_core::{
    ISLAND_ICON, ISLAND_PAD, LAUNCHER_CARD_W, LAUNCHER_ROW_H, LAUNCHER_Y, LauncherGroup,
    display_name, island_caption, island_caption_y, island_card_h, island_card_y, island_chevron_x,
    island_chevron_y, island_icon_y, island_text_end, island_text_x, island_title_y,
    system_caption,
};

use crate::apps::Apps;
use crate::draw::LcdDraw;
use crate::st7789::{HEIGHT, St7789, WIDTH};

const INSET: u16 = 16;
const GROUP_W: u16 = LAUNCHER_CARD_W;
const MENU_Y0: u16 = 30;
const MENU_ROW: u16 = 22;
const MENU_GAP: u16 = 8;
const ICON: u16 = 16;
const FONT_W: u16 = 6;

/// Boot / dark page fill. Same as [`Palette::DARK`].bg — proven on this panel.
pub const COL_BG: u16 = 0x10A2;

const MENU_GROUPS: &[(usize, usize)] = &[(0, 3), (3, 3), (6, 2), (8, 3)];
const WIFI_Y0: u16 = 30;
const WIFI_ROW: u16 = 22;
const IME_SSID_Y: u16 = 30;
const IME_FIELD_Y: u16 = 50;
const IME_FIELD_H: u16 = 28;
const IME_KEY_Y: u16 = 84;
const IME_KEY_H: u16 = 22;

pub fn paint_boot<SPI, DC, CS, E>(lcd: &mut St7789<SPI, DC, CS>, anim: &BootAnim) -> Result<(), E>
where
    SPI: embedded_hal::spi::SpiBus<u8, Error = E>,
    DC: embedded_hal::digital::OutputPin,
    CS: embedded_hal::digital::OutputPin,
{
    let vp = Rect {
        x: 0,
        y: 0,
        w: WIDTH,
        h: HEIGHT,
    };
    let mut draw = LcdDraw::new(lcd, vp);
    anim.paint(&mut draw, Palette::DARK);
    Ok(())
}

pub fn overlay_name(overlay: Overlay) -> &'static str {
    match overlay {
        Overlay::Launcher => "launcher",
        Overlay::System => "system",
        Overlay::Keys => "keys",
        Overlay::About => "about",
        Overlay::Wifi => "wifi",
        Overlay::None => "desk",
    }
}

pub fn pal(shell: &Shell) -> Palette {
    shell.theme().palette()
}

pub fn paint<SPI, DC, CS, E>(
    lcd: &mut St7789<SPI, DC, CS>,
    shell: &Shell,
    apps: &Apps,
    prev: &mut Option<FrameSig>,
) -> Result<(), E>
where
    SPI: embedded_hal::spi::SpiBus<u8, Error = E>,
    DC: embedded_hal::digital::OutputPin,
    CS: embedded_hal::digital::OutputPin,
    St7789<SPI, DC, CS>: FillDraw<E>,
{
    let now = FrameSig::capture(shell);
    let plan = PaintPlan::diff(*prev, now);
    *prev = Some(now);
    if plan.is_nop() {
        return Ok(());
    }
    let p = pal(shell);
    if plan.wipe_content {
        fill_page(lcd, STATUS_BAR_H, HEIGHT - STATUS_BAR_H, p, shell.theme())?;
    }
    if plan.status {
        paint_status(lcd, shell, p)?;
    }
    if plan.desktop {
        paint_desktop_body(lcd, shell, p)?;
    }
    if let Some((a, b)) = plan.launcher_cards {
        if shell.launcher().is_islands() {
            paint_one_island(lcd, shell, p, a)?;
            if b != a {
                paint_one_island(lcd, shell, p, b)?;
            }
        } else {
            paint_launcher_row(lcd, shell, p, a)?;
            if b != a {
                paint_launcher_row(lcd, shell, p, b)?;
            }
        }
    }
    if plan.control {
        paint_control_body(lcd, shell, p)?;
    }
    if let Some((a, b)) = plan.menu_rows {
        paint_menu_row(lcd, shell, p, a)?;
        if b != a {
            paint_menu_row(lcd, shell, p, b)?;
        }
    }
    if plan.keys {
        paint_keys_body(lcd, p)?;
    }
    if plan.about {
        paint_about_body(lcd, shell, p)?;
    }
    if plan.wifi {
        paint_wifi_body(lcd, shell, p)?;
    } else {
        if let Some((a, b)) = plan.wifi_rows {
            paint_wifi_row(lcd, shell, p, a)?;
            if b != a {
                paint_wifi_row(lcd, shell, p, b)?;
            }
        }
        if plan.ime_field {
            paint_ime_field(lcd, shell, p)?;
        }
        if let Some((a, b)) = plan.ime_keys {
            paint_ime_key(lcd, shell, p, a)?;
            if b != a {
                paint_ime_key(lcd, shell, p, b)?;
            }
        }
    }
    if plan.pulse || plan.game {
        paint_workspace(lcd, shell, apps, p, true)?;
    } else if plan.game_live || plan.pulse_meter {
        paint_workspace(lcd, shell, apps, p, false)?;
    }
    Ok(())
}

fn fill_page<LCD, E>(lcd: &mut LCD, y: u16, h: u16, p: Palette, theme: Theme) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    if theme.is_light() {
        let mut yy = y;
        let end = y.saturating_add(h).min(HEIGHT);
        while yy < end {
            let hh = 8.min(end - yy);
            lcd.fill_rect(0, yy, WIDTH, hh, p.bg)?;
            yy = yy.saturating_add(hh);
        }
        Ok(())
    } else {
        lcd.fill_rect(0, y, WIDTH, h, p.bg)
    }
}

fn paint_status<LCD, E>(lcd: &mut LCD, shell: &Shell, p: Palette) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    lcd.fill_rect(0, 0, WIDTH, STATUS_BAR_H, p.bg)?;
    let time = shell.status.clock.format_hm();
    lcd.draw_text(INSET, 7, time.as_str(), p.label, p.bg)?;
    let radio = match shell.status.radio {
        RadioMode::Off => "",
        RadioMode::Wifi => "Wi-Fi",
        RadioMode::Ble => "BT",
    };
    if !radio.is_empty() {
        lcd.draw_text(INSET + 5 * FONT_W + 8, 7, radio, p.accent, p.bg)?;
    }
    for i in 0u16..2 {
        let x = 112 + i * 10;
        let on = u16::from(shell.status.workspace) == i;
        lcd.fill_rect(x, 9, 5, 5, if on { p.accent } else { p.separator })?;
    }

    let soc = shell.status.battery_soc;
    let bat_col = match soc {
        Some(n) if n <= 15 => p.low,
        Some(_) => p.ok,
        None => p.secondary,
    };
    let mut pct = heapless::String::<8>::new();
    match soc {
        Some(n) => {
            let _ = write!(pct, "{n}%");
        }
        None => {
            let _ = pct.push_str("--");
        }
    }
    let pct_w = (pct.len() as u16).saturating_mul(FONT_W);
    let bat_x = WIDTH - INSET - 20;
    let text_x = bat_x.saturating_sub(8).saturating_sub(pct_w);
    lcd.draw_text(text_x, 7, pct.as_str(), p.label, p.bg)?;
    lcd.fill_rect(bat_x, 8, 16, 7, p.separator)?;
    lcd.fill_rect(bat_x + 16, 10, 2, 3, p.separator)?;
    let fill = match soc {
        Some(n) => ((u16::from(n) * 12) / 100).max(1),
        None => 0,
    };
    if fill > 0 {
        lcd.fill_rect(bat_x + 1, 9, fill.min(14), 5, bat_col)?;
    }
    if shell.status.charging {
        paint_bolt(lcd, bat_x + 5, 8, p.accent, p.bg)?;
    }
    lcd.fill_rect(INSET, STATUS_BAR_H - 1, WIDTH - INSET * 2, 1, p.separator)?;
    Ok(())
}

/// 5×7 lightning over the battery body (1x fill_rect, no glyphs).
fn paint_bolt<LCD, E>(lcd: &mut LCD, x: u16, y: u16, fg: u16, _bg: u16) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    lcd.fill_rect(x + 3, y, 2, 1, fg)?;
    lcd.fill_rect(x + 2, y + 1, 3, 1, fg)?;
    lcd.fill_rect(x + 1, y + 2, 3, 1, fg)?;
    lcd.fill_rect(x, y + 3, 4, 1, fg)?;
    lcd.fill_rect(x + 2, y + 4, 3, 1, fg)?;
    lcd.fill_rect(x + 3, y + 5, 2, 1, fg)?;
    lcd.fill_rect(x + 1, y + 6, 2, 1, fg)?;
    Ok(())
}

fn paint_desktop_body<LCD, E>(lcd: &mut LCD, shell: &Shell, p: Palette) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    if shell.launcher().is_islands() {
        return paint_islands(lcd, shell, p);
    }
    let (start, len) = shell.launcher_window();
    if len == 0 {
        return Ok(());
    }
    let gh = (len as u16).saturating_mul(LAUNCHER_ROW_H);
    lcd.fill_rect(INSET, LAUNCHER_Y, GROUP_W, gh, p.grouped)?;
    for i in start..start + len {
        paint_launcher_row(lcd, shell, p, i)?;
    }
    Ok(())
}

fn paint_islands<LCD, E>(lcd: &mut LCD, shell: &Shell, p: Palette) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    let n = shell.launcher_names().len();
    for i in 0..n {
        paint_one_island(lcd, shell, p, i)?;
    }
    Ok(())
}

fn paint_one_island<LCD, E>(lcd: &mut LCD, shell: &Shell, p: Palette, i: usize) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    let names = shell.launcher_names();
    let Some(name) = names.get(i).copied() else {
        return Ok(());
    };
    let slots = shell.registry.slots();
    let card_h = island_card_h();
    let y = island_card_y(i);
    if y.saturating_add(card_h) > HEIGHT {
        return Ok(());
    }
    let text_x = INSET + island_text_x();
    let chev_x = INSET + island_chevron_x();
    let text_right = INSET + island_text_end();
    let focused = i == shell.launcher().selected;
    let bg = if focused { p.grouped_sel } else { p.grouped };
    lcd.fill_rect(INSET, y, GROUP_W, card_h, bg)?;
    let title = display_name(name);
    let mut caption = heapless::String::<24>::new();
    match name {
        "play" => {
            let _ = caption.push_str(island_caption(LauncherGroup::Play, slots).as_str());
        }
        "tools" => {
            let _ = caption.push_str(island_caption(LauncherGroup::Tools, slots).as_str());
        }
        "system" => {
            let _ = caption.push_str(system_caption());
        }
        other => {
            let _ = caption.push_str(display_name(other));
        }
    }
    paint_island_mark(lcd, name, INSET + ISLAND_PAD, island_icon_y(y), bg, p)?;
    draw_text_fit(
        lcd,
        text_x,
        island_title_y(y),
        title,
        p.label,
        bg,
        text_right,
        2,
    )?;
    draw_text_fit(
        lcd,
        text_x,
        island_caption_y(y),
        caption.as_str(),
        p.secondary,
        bg,
        text_right,
        1,
    )?;
    lcd.draw_text(chev_x, island_chevron_y(y), ">", p.secondary, bg)?;
    Ok(())
}

/// 22px marks, not a letter on a blob. Play is the four games.
fn paint_island_mark<LCD, E>(
    lcd: &mut LCD,
    name: &str,
    x: u16,
    y: u16,
    bg: u16,
    p: Palette,
) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    match name {
        "play" => {
            lcd.fill_rect(x, y, 10, 10, 0xFE60)?;
            lcd.fill_rect(x + 12, y, 10, 10, 0x07FD)?;
            lcd.fill_rect(x, y + 12, 10, 10, 0xF800)?;
            lcd.fill_rect(x + 12, y + 12, 10, 10, 0xC618)?;
        }
        "tools" => {
            lcd.fill_rect(x + 2, y + 14, 3, 8, p.icon_pulse)?;
            lcd.fill_rect(x + 7, y + 8, 3, 14, p.icon_pulse)?;
            lcd.fill_rect(x + 12, y + 2, 3, 20, p.icon_pulse)?;
            lcd.fill_rect(x + 17, y + 6, 3, 16, p.icon_pulse)?;
        }
        "system" => {
            lcd.fill_rect(x + 7, y, 8, 22, p.icon_system)?;
            lcd.fill_rect(x, y + 7, 22, 8, p.icon_system)?;
            lcd.fill_rect(x + 4, y + 4, 14, 14, p.icon_system)?;
            lcd.fill_rect(x + 8, y + 8, 6, 6, bg)?;
        }
        _ => {
            lcd.fill_rect(x, y, ISLAND_ICON, ISLAND_ICON, p.accent)?;
        }
    }
    Ok(())
}

fn card_text_end() -> u16 {
    INSET + GROUP_W - ISLAND_PAD
}

fn draw_text_fit<LCD, E>(
    lcd: &mut LCD,
    mut x: u16,
    y: u16,
    text: &str,
    fg: u16,
    bg: u16,
    x_end: u16,
    scale: u16,
) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    let cell = FONT_W.saturating_mul(scale.max(1));
    for b in text.bytes() {
        if !(32..127).contains(&b) {
            continue;
        }
        if x.saturating_add(cell) > x_end {
            break;
        }
        let tmp = [b];
        let s = core::str::from_utf8(&tmp).unwrap_or("");
        if scale >= 2 {
            lcd.draw_text_2x(x, y, s, fg, bg)?;
        } else {
            lcd.draw_text(x, y, s, fg, bg)?;
        }
        x = x.saturating_add(cell);
    }
    Ok(())
}

fn paint_launcher_row<LCD, E>(lcd: &mut LCD, shell: &Shell, p: Palette, i: usize) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    let names = shell.launcher_names();
    let Some(name) = names.get(i).copied() else {
        return Ok(());
    };
    let (start, len) = shell.launcher_window();
    if i < start || i >= start + len {
        return Ok(());
    }
    let vis = (i - start) as u16;
    let y = LAUNCHER_Y.saturating_add(vis.saturating_mul(LAUNCHER_ROW_H));
    if y.saturating_add(LAUNCHER_ROW_H) > HEIGHT {
        return Ok(());
    }
    let focused = i == shell.launcher().selected;
    let bg = if focused { p.grouped_sel } else { p.grouped };
    lcd.fill_rect(INSET, y, GROUP_W, LAUNCHER_ROW_H, bg)?;
    let label = display_name(name);
    let icon = match name {
        "pulse" => p.icon_pulse,
        "nfc" => p.icon_tap,
        "flap" => 0xFE60,
        "stack" => 0x07FD,
        "brick" => 0xF800,
        "boo" => 0xC618,
        "tune" => 0x07E0,
        "system" => p.icon_system,
        _ => p.accent,
    };
    let ix = INSET + 10;
    let iy = y + (LAUNCHER_ROW_H - ICON) / 2;
    lcd.fill_rect(ix, iy, ICON, ICON, icon)?;
    let ch = match name {
        "pulse" => "P",
        "nfc" => "N",
        "flap" => "F",
        "stack" => "K",
        "brick" => "B",
        "boo" => "O",
        "tune" => "T",
        "system" => "S",
        _ => "",
    };
    if !ch.is_empty() {
        lcd.draw_text(ix + 5, iy + 4, ch, bg, icon)?;
    }
    let chev_x = INSET + GROUP_W - 12 - FONT_W;
    draw_text_fit(
        lcd,
        INSET + 36,
        y + 18,
        label,
        p.label,
        bg,
        chev_x.saturating_sub(6),
        1,
    )?;
    lcd.draw_text(chev_x, y + 18, ">", p.secondary, bg)?;
    if vis + 1 < len as u16 {
        lcd.fill_rect(
            INSET + 40,
            y + LAUNCHER_ROW_H - 1,
            GROUP_W - 52,
            1,
            p.separator,
        )?;
    }
    Ok(())
}

fn menu_row_y(i: usize) -> u16 {
    let mut y = MENU_Y0;
    for &(start, count) in MENU_GROUPS {
        if i >= start && i < start + count {
            return y + ((i - start) as u16) * MENU_ROW;
        }
        y = y
            .saturating_add((count as u16) * MENU_ROW)
            .saturating_add(MENU_GAP);
    }
    y
}

fn paint_control_body<LCD, E>(lcd: &mut LCD, shell: &Shell, p: Palette) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    let mut y = MENU_Y0;
    for &(start, count) in MENU_GROUPS {
        let h = (count as u16) * MENU_ROW;
        lcd.fill_rect(INSET, y, GROUP_W, h, p.grouped)?;
        for j in 0..count {
            paint_menu_row(lcd, shell, p, start + j)?;
            if j + 1 < count {
                let sy = y + (j as u16 + 1) * MENU_ROW;
                lcd.fill_rect(INSET + 14, sy, GROUP_W - 28, 1, p.separator)?;
            }
        }
        y = y.saturating_add(h).saturating_add(MENU_GAP);
    }
    Ok(())
}

fn paint_menu_row<LCD, E>(lcd: &mut LCD, shell: &Shell, p: Palette, i: usize) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    let Some(item) = MENU_ITEMS.get(i) else {
        return Ok(());
    };
    let y = menu_row_y(i);
    if y.saturating_add(MENU_ROW) > HEIGHT {
        return Ok(());
    }
    let focused = i == shell.menu_selected();
    let bg = if focused { p.grouped_sel } else { p.grouped };
    lcd.fill_rect(INSET, y, GROUP_W, MENU_ROW, bg)?;
    let acc = accessory(shell, item.action);
    let mut label_end = card_text_end();
    if !acc.is_empty() {
        let aw = (acc.len() as u16).saturating_mul(FONT_W);
        let acc_x = INSET + GROUP_W - 12 - aw;
        draw_text_fit(lcd, acc_x, y + 7, acc, p.secondary, bg, card_text_end(), 1)?;
        label_end = acc_x.saturating_sub(6);
    }
    draw_text_fit(
        lcd,
        INSET + 12,
        y + 7,
        item.label,
        p.label,
        bg,
        label_end,
        1,
    )?;
    Ok(())
}

fn accessory(shell: &Shell, action: MenuAction) -> &'static str {
    match action {
        MenuAction::ThemeToggle => shell.theme().as_str(),
        MenuAction::RadioOff if shell.status.radio == RadioMode::Off => "*",
        MenuAction::RadioWifi if shell.status.radio == RadioMode::Wifi => "*",
        MenuAction::RadioWifi => ">",
        MenuAction::RadioBle if shell.status.radio == RadioMode::Ble => "*",
        MenuAction::Keys | MenuAction::About => ">",
        _ => "",
    }
}

fn paint_keys_body<LCD, E>(lcd: &mut LCD, p: Palette) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    let y = STATUS_BAR_H + 16;
    let h = (CHEATSHEET.len() as u16).saturating_mul(18) + 16;
    lcd.fill_rect(INSET, y, GROUP_W, h, p.grouped)?;
    let mut ty = y + 10;
    for line in CHEATSHEET {
        draw_text_fit(
            lcd,
            INSET + 12,
            ty,
            line,
            p.label,
            p.grouped,
            card_text_end(),
            1,
        )?;
        ty = ty.saturating_add(18);
    }
    Ok(())
}

fn paint_about_body<LCD, E>(lcd: &mut LCD, shell: &Shell, p: Palette) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    if shell.about_page() == ABOUT_PAGE_STORAGE {
        paint_storage_body(lcd, shell, p)
    } else {
        paint_about_card(lcd, STATUS_BAR_H + 16, &ABOUT[0..3], p)?;
        paint_about_card(lcd, 118, &ABOUT[3..6], p)?;
        paint_about_card(lcd, 210, &ABOUT[6..9], p)?;
        Ok(())
    }
}

fn paint_storage_body<LCD, E>(lcd: &mut LCD, shell: &Shell, p: Palette) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    let used = shell.factory_used();
    let free = shell.factory_free();
    let used_s = used.map(format_size);
    let free_s = free.map(format_size);
    let used_line = match used_s.as_ref() {
        Some(s) => {
            let mut l = heapless::String::<24>::new();
            let _ = write!(l, "used  {}", s.as_str());
            l
        }
        None => {
            let mut l = heapless::String::<24>::new();
            let _ = l.push_str("used  --");
            l
        }
    };
    let free_line = match free_s.as_ref() {
        Some(s) => {
            let mut l = heapless::String::<24>::new();
            let _ = write!(l, "free  {}", s.as_str());
            l
        }
        None => {
            let mut l = heapless::String::<24>::new();
            let _ = l.push_str("free  --");
            l
        }
    };
    let y0 = STATUS_BAR_H + 16;
    lcd.fill_rect(INSET, y0, GROUP_W, 100, p.grouped)?;
    draw_text_fit(
        lcd,
        INSET + 12,
        y0 + 12,
        "Storage",
        p.accent,
        p.grouped,
        card_text_end(),
        1,
    )?;
    draw_text_fit(
        lcd,
        INSET + 12,
        y0 + 32,
        used_line.as_str(),
        p.label,
        p.grouped,
        card_text_end(),
        1,
    )?;
    draw_text_fit(
        lcd,
        INSET + 12,
        y0 + 50,
        free_line.as_str(),
        p.label,
        p.grouped,
        card_text_end(),
        1,
    )?;
    let bar_x = INSET + 12;
    let bar_y = y0 + 74;
    let bar_w = GROUP_W - 24;
    lcd.fill_rect(bar_x, bar_y, bar_w, 8, p.separator)?;
    if let Some(n) = used {
        let fill = used_bar_width(n, bar_w);
        if fill > 0 {
            lcd.fill_rect(bar_x, bar_y, fill, 8, p.accent)?;
        }
    }

    let mut factory = heapless::String::<24>::new();
    let _ = write!(factory, "factory {}", format_size(factory_total()).as_str());
    let mut flash = heapless::String::<24>::new();
    let _ = write!(flash, "flash   {}", format_size(FLASH_SIZE).as_str());
    let mut kv = heapless::String::<24>::new();
    let _ = write!(kv, "kv      {}", format_size(FLASH_KV_SIZE).as_str());
    paint_about_card(
        lcd,
        150,
        &[flash.as_str(), factory.as_str(), kv.as_str()],
        p,
    )?;
    paint_about_card(lcd, 240, &["UP/DN  page", "OK     back"], p)?;
    Ok(())
}

fn paint_about_card<LCD, E>(lcd: &mut LCD, y: u16, lines: &[&str], p: Palette) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    lcd.fill_rect(INSET, y, GROUP_W, 78, p.grouped)?;
    let mut ty = y + 14;
    for (i, line) in lines.iter().enumerate() {
        let fg = if i == 0 { p.accent } else { p.label };
        draw_text_fit(lcd, INSET + 12, ty, line, fg, p.grouped, card_text_end(), 1)?;
        ty = ty.saturating_add(18);
    }
    Ok(())
}

fn paint_wifi_body<LCD, E>(lcd: &mut LCD, shell: &Shell, p: Palette) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    match shell.wifi().phase() {
        WifiPhase::Scan => paint_wifi_msg(lcd, p, "scan"),
        WifiPhase::Connecting => paint_wifi_msg(lcd, p, "join"),
        WifiPhase::Result => {
            let msg = if shell.wifi().result_ok() {
                "ok"
            } else {
                "fail"
            };
            paint_wifi_msg(lcd, p, msg)
        }
        WifiPhase::Ime => paint_ime_body(lcd, shell, p),
        WifiPhase::List => paint_wifi_list(lcd, shell, p),
    }
}

fn paint_wifi_msg<LCD, E>(lcd: &mut LCD, p: Palette, msg: &str) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    lcd.fill_rect(INSET, WIFI_Y0, GROUP_W, 44, p.grouped)?;
    draw_text_fit(
        lcd,
        INSET + 12,
        WIFI_Y0 + 18,
        msg,
        p.label,
        p.grouped,
        card_text_end(),
        1,
    )
}

fn paint_wifi_list<LCD, E>(lcd: &mut LCD, shell: &Shell, p: Palette) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    let wifi = shell.wifi();
    let (start, len) = wifi.window(WIFI_VISIBLE);
    if len == 0 {
        return Ok(());
    }
    let h = (len as u16).saturating_mul(WIFI_ROW);
    lcd.fill_rect(INSET, WIFI_Y0, GROUP_W, h, p.grouped)?;
    for i in start..start + len {
        paint_wifi_row(lcd, shell, p, i)?;
        if i + 1 < start + len {
            let vis = (i - start) as u16;
            let sy = WIFI_Y0 + (vis + 1) * WIFI_ROW;
            lcd.fill_rect(INSET + 14, sy, GROUP_W - 28, 1, p.separator)?;
        }
    }
    Ok(())
}

fn paint_wifi_row<LCD, E>(lcd: &mut LCD, shell: &Shell, p: Palette, i: usize) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    let wifi = shell.wifi();
    let (start, len) = wifi.window(WIFI_VISIBLE);
    if i < start || i >= start + len {
        return Ok(());
    }
    let vis = (i - start) as u16;
    let y = WIFI_Y0 + vis * WIFI_ROW;
    if y.saturating_add(WIFI_ROW) > HEIGHT {
        return Ok(());
    }
    let focused = i == wifi.selected();
    let bg = if focused { p.grouped_sel } else { p.grouped };
    lcd.fill_rect(INSET, y, GROUP_W, WIFI_ROW, bg)?;
    if wifi.row_is_scan(i) {
        draw_text_fit(
            lcd,
            INSET + 12,
            y + 7,
            "scan",
            p.accent,
            bg,
            card_text_end(),
            1,
        )?;
        return Ok(());
    }
    if let Some(net) = wifi.net_at(i) {
        let ssid_end = if net.open {
            INSET + GROUP_W - 26
        } else {
            INSET + GROUP_W - 40
        };
        draw_text_fit(
            lcd,
            INSET + 12,
            y + 7,
            net.ssid.as_str(),
            p.label,
            bg,
            ssid_end,
            1,
        )?;
        if !net.open {
            lcd.draw_text(INSET + GROUP_W - 36, y + 7, "#", p.secondary, bg)?;
        }
        paint_rssi(lcd, INSET + GROUP_W - 22, y + 6, net.rssi, p.accent, bg)?;
    }
    Ok(())
}

fn paint_rssi<LCD, E>(lcd: &mut LCD, x: u16, y: u16, rssi: i8, fg: u16, bg: u16) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    let bars = if rssi >= -55 {
        3
    } else if rssi >= -70 {
        2
    } else {
        1
    };
    for i in 0..3u16 {
        let h = 3 + i * 2;
        let col = if i < bars { fg } else { bg };
        lcd.fill_rect(x + i * 4, y + 8 - h, 3, h, col)?;
    }
    Ok(())
}

fn paint_ime_body<LCD, E>(lcd: &mut LCD, shell: &Shell, p: Palette) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    draw_text_fit(
        lcd,
        INSET + 4,
        IME_SSID_Y + 4,
        shell.wifi().pending_ssid(),
        p.secondary,
        p.bg,
        WIDTH - INSET,
        1,
    )?;
    paint_ime_field(lcd, shell, p)?;
    let n = IME_ROWS.iter().map(|r| r.len()).sum::<usize>();
    for i in 0..n {
        paint_ime_key(lcd, shell, p, i)?;
    }
    Ok(())
}

fn paint_ime_field<LCD, E>(lcd: &mut LCD, shell: &Shell, p: Palette) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    lcd.fill_rect(INSET, IME_FIELD_Y, GROUP_W, IME_FIELD_H, p.grouped)?;
    let masked = shell.wifi().ime().masked();
    draw_text_fit(
        lcd,
        INSET + 12,
        IME_FIELD_Y + 10,
        masked.as_str(),
        p.label,
        p.grouped,
        card_text_end(),
        1,
    )
}

fn ime_key_geom(idx: usize) -> Option<(u16, u16, u16, u16)> {
    let mut n = 0usize;
    let mut y = IME_KEY_Y;
    for row in IME_ROWS {
        let cols = row.len() as u16;
        if cols == 0 {
            continue;
        }
        let w = GROUP_W / cols;
        for col in 0..row.len() {
            if n == idx {
                return Some((INSET + (col as u16) * w, y, w, IME_KEY_H));
            }
            n += 1;
        }
        y = y.saturating_add(IME_KEY_H);
    }
    None
}

fn paint_ime_key<LCD, E>(lcd: &mut LCD, shell: &Shell, p: Palette, idx: usize) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    let Some((x, y, w, h)) = ime_key_geom(idx) else {
        return Ok(());
    };
    if y.saturating_add(h) > HEIGHT {
        return Ok(());
    }
    let ime = shell.wifi().ime();
    let focused = idx == ime.cursor();
    let bg = if focused { p.grouped_sel } else { p.grouped };
    lcd.fill_rect(x, y, w, h, bg)?;
    let key = passport_core::ime::key_at(idx);
    let mut label = heapless::String::<4>::new();
    key.write_label(ime.shift(), &mut label);
    let tw = (label.len() as u16).saturating_mul(FONT_W);
    let tx = x + w.saturating_sub(tw) / 2;
    let ty = y + 7;
    let fg = if matches!(key, ImeKey::Done) {
        p.accent
    } else {
        p.label
    };
    draw_text_fit(lcd, tx, ty, label.as_str(), fg, bg, x.saturating_add(w), 1)
}

fn paint_workspace<SPI, DC, CS, E>(
    lcd: &mut St7789<SPI, DC, CS>,
    shell: &Shell,
    apps: &Apps,
    p: Palette,
    full: bool,
) -> Result<(), E>
where
    SPI: embedded_hal::spi::SpiBus<u8, Error = E>,
    DC: embedded_hal::digital::OutputPin,
    CS: embedded_hal::digital::OutputPin,
    St7789<SPI, DC, CS>: FillDraw<E>,
{
    let layout = shell.layout();
    if layout.tiles.is_empty() {
        return Ok(());
    }
    match shell.focused_app_name() {
        Some("nfc") => {
            if full {
                crate::apps::nfc::paint(lcd, &apps.nfc, p);
            }
        }
        Some("flap") => {
            crate::apps::flap::paint(lcd, &apps.flap, p, !full);
        }
        Some("stack") => {
            crate::apps::stack::paint(lcd, &apps.stack, p, !full);
        }
        Some("brick") => {
            crate::apps::brick::paint(lcd, &apps.brick, p, !full);
        }
        Some("boo") => {
            crate::apps::boo::paint(lcd, &apps.boo, p, !full);
        }
        Some("tune") => {
            crate::apps::tune::paint(lcd, &apps.tune, p, !full);
        }
        _ => {
            if full {
                crate::apps::pulse::paint(lcd, &apps.pulse, p);
            } else {
                crate::apps::pulse::paint_meter(lcd, &apps.pulse, p);
            }
        }
    }
    Ok(())
}

/// Drawing surface implemented by [`St7789`].
pub trait FillDraw<E> {
    fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, color: u16) -> Result<(), E>;
    fn draw_text(&mut self, x: u16, y: u16, text: &str, fg: u16, bg: u16) -> Result<(), E>;
    fn draw_text_2x(&mut self, x: u16, y: u16, text: &str, fg: u16, bg: u16) -> Result<(), E>;
}

impl<SPI, DC, CS, E> FillDraw<E> for St7789<SPI, DC, CS>
where
    SPI: embedded_hal::spi::SpiBus<u8, Error = E>,
    DC: embedded_hal::digital::OutputPin,
    CS: embedded_hal::digital::OutputPin,
{
    fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, color: u16) -> Result<(), E> {
        St7789::fill_rect(self, x, y, w, h, color)
    }
    fn draw_text(&mut self, x: u16, y: u16, text: &str, fg: u16, bg: u16) -> Result<(), E> {
        St7789::draw_text(self, x, y, text, fg, bg)
    }
    fn draw_text_2x(&mut self, x: u16, y: u16, text: &str, fg: u16, bg: u16) -> Result<(), E> {
        St7789::draw_text_2x(self, x, y, text, fg, bg)
    }
}
