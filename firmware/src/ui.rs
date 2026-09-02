//! Apple grouped-table chrome. Dark is the proven invert-on field; light page
//! fills are 8-row bands (a single 240×320 white RAMWR painted as 花屏).

use core::fmt::Write as _;

use passport_core::boot::BootAnim;
use passport_core::compositor::{Rect, STATUS_BAR_H};
use passport_core::ime::{ImeKey, IME_ROWS};
use passport_core::keymap::{ABOUT, CHEATSHEET};
use passport_core::menu::{MenuAction, MENU_ITEMS};
use passport_core::paint::{FrameSig, PaintPlan};
use passport_core::shell::{Overlay, Shell};
use passport_core::status::RadioMode;
use passport_core::theme::Palette;
use passport_core::wifi::{WifiPhase, WIFI_VISIBLE};
use passport_core::Theme;

use crate::apps::Apps;
use crate::draw::LcdDraw;
use crate::st7789::{St7789, HEIGHT, WIDTH};

const INSET: u16 = 16;
const GROUP_W: u16 = 208;
const LAUNCH_Y: u16 = 40;
const LAUNCH_ROW: u16 = 44;
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

pub fn paint_boot<SPI, DC, CS, E>(
    lcd: &mut St7789<SPI, DC, CS>,
    anim: &BootAnim,
) -> Result<(), E>
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
        paint_launcher_row(lcd, shell, p, a)?;
        if b != a {
            paint_launcher_row(lcd, shell, p, b)?;
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
        paint_about_body(lcd, p)?;
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
    let n = shell.launcher_names().len();
    if n == 0 {
        return Ok(());
    }
    let gh = (n as u16).saturating_mul(LAUNCH_ROW);
    lcd.fill_rect(INSET, LAUNCH_Y, GROUP_W, gh, p.grouped)?;
    for i in 0..n {
        paint_launcher_row(lcd, shell, p, i)?;
        if i + 1 < n {
            let sy = LAUNCH_Y + (i as u16 + 1) * LAUNCH_ROW;
            lcd.fill_rect(INSET + 40, sy, GROUP_W - 52, 1, p.separator)?;
        }
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
    let y = LAUNCH_Y.saturating_add((i as u16).saturating_mul(LAUNCH_ROW));
    if y.saturating_add(LAUNCH_ROW) > HEIGHT {
        return Ok(());
    }
    let focused = i == shell.launcher().selected;
    let bg = if focused { p.grouped_sel } else { p.grouped };
    lcd.fill_rect(INSET, y, GROUP_W, LAUNCH_ROW, bg)?;
    let n = names.len();
    let (label, icon) = match name {
        "pulse" => ("Pulse", p.icon_pulse),
        "nfc" => ("Tap", p.icon_tap),
        "flap" => ("Flap", 0xFE60),
        "stack" => ("Stack", 0x07FD),
        "brick" => ("Brick", 0xF800),
        "system" => ("System", p.icon_system),
        other => (other, p.accent),
    };
    let ix = INSET + 10;
    let iy = y + (LAUNCH_ROW - ICON) / 2;
    lcd.fill_rect(ix, iy, ICON, ICON, icon)?;
    let ch = match name {
        "pulse" => "P",
        "nfc" => "N",
        "flap" => "F",
        "stack" => "K",
        "brick" => "B",
        "system" => "S",
        _ => "",
    };
    if !ch.is_empty() {
        lcd.draw_text(ix + 5, iy + 4, ch, p.grouped, icon)?;
    }
    lcd.draw_text(INSET + 36, y + 18, label, p.label, bg)?;
    lcd.draw_text(INSET + GROUP_W - 16, y + 18, ">", p.secondary, bg)?;
    if i + 1 < n {
        lcd.fill_rect(INSET + 40, y + LAUNCH_ROW - 1, GROUP_W - 52, 1, p.separator)?;
    }
    Ok(())
}

fn menu_row_y(i: usize) -> u16 {
    let mut y = MENU_Y0;
    for &(start, count) in MENU_GROUPS {
        if i >= start && i < start + count {
            return y + ((i - start) as u16) * MENU_ROW;
        }
        y = y.saturating_add((count as u16) * MENU_ROW).saturating_add(MENU_GAP);
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
    lcd.draw_text(INSET + 12, y + 7, item.label, p.label, bg)?;
    let acc = accessory(shell, item.action);
    if !acc.is_empty() {
        let aw = (acc.len() as u16).saturating_mul(FONT_W);
        lcd.draw_text(INSET + GROUP_W - 12 - aw, y + 7, acc, p.secondary, bg)?;
    }
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
        lcd.draw_text(INSET + 12, ty, line, p.label, p.grouped)?;
        ty = ty.saturating_add(18);
    }
    Ok(())
}

fn paint_about_body<LCD, E>(lcd: &mut LCD, p: Palette) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    paint_about_card(lcd, STATUS_BAR_H + 16, &ABOUT[0..3], p)?;
    paint_about_card(lcd, 118, &ABOUT[3..6], p)?;
    paint_about_card(lcd, 210, &ABOUT[6..9], p)?;
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
        lcd.draw_text(INSET + 12, ty, line, fg, p.grouped)?;
        ty = ty.saturating_add(18);
    }
    Ok(())
}

fn trunc(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

fn paint_wifi_body<LCD, E>(lcd: &mut LCD, shell: &Shell, p: Palette) -> Result<(), E>
where
    LCD: FillDraw<E>,
{
    match shell.wifi().phase() {
        WifiPhase::Scan => paint_wifi_msg(lcd, p, "scan"),
        WifiPhase::Connecting => paint_wifi_msg(lcd, p, "join"),
        WifiPhase::Result => {
            let msg = if shell.wifi().result_ok() { "ok" } else { "fail" };
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
    lcd.draw_text(INSET + 12, WIFI_Y0 + 18, msg, p.label, p.grouped)
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
        lcd.draw_text(INSET + 12, y + 7, "scan", p.accent, bg)?;
        return Ok(());
    }
    if let Some(net) = wifi.net_at(i) {
        lcd.draw_text(INSET + 12, y + 7, trunc(net.ssid.as_str(), 22), p.label, bg)?;
        if !net.open {
            lcd.draw_text(INSET + GROUP_W - 36, y + 7, "#", p.secondary, bg)?;
        }
        paint_rssi(lcd, INSET + GROUP_W - 22, y + 6, net.rssi, p.accent, bg)?;
    }
    Ok(())
}

fn paint_rssi<LCD, E>(
    lcd: &mut LCD,
    x: u16,
    y: u16,
    rssi: i8,
    fg: u16,
    bg: u16,
) -> Result<(), E>
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
    let ssid = trunc(shell.wifi().pending_ssid(), 28);
    lcd.draw_text(INSET + 4, IME_SSID_Y + 4, ssid, p.secondary, p.bg)?;
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
    lcd.draw_text(
        INSET + 12,
        IME_FIELD_Y + 10,
        trunc(masked.as_str(), 28),
        p.label,
        p.grouped,
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
    lcd.draw_text(tx, ty, label.as_str(), fg, bg)
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
}
