#![no_std]
#![no_main]

extern crate alloc;

mod apps;
mod codec;
mod draw;
mod font;
mod st7789;
mod store;
mod ui;

use core::fmt::Write as _;

use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Timer};
use embedded_hal::delay::DelayNs;
use embedded_io_async::{Read, Write as AsyncWrite};
use esp_backtrace as _;
use esp_hal::{
    analog::adc::{Adc, AdcCalCurve, AdcConfig, Attenuation},
    clock::CpuClock,
    delay::Delay,
    dma_rx_stream_buffer, dma_tx_stream_buffer,
    gpio::{DriveMode, Level, Output, OutputConfig},
    i2c::master::{Config as I2cConfig, I2c},
    i2s::master::{Channels, DataFormat, I2s, TdmConfig},
    ledc::{
        LSGlobalClkSource, Ledc, LowSpeed,
        channel::{self, ChannelIFace},
        timer::{self, TimerIFace},
    },
    rtc_cntl::sleep::{LowPower, RtcSleepConfig},
    spi::{
        Mode,
        master::{Config as SpiConfig, Spi},
    },
    time::Rate,
    timer::timg::TimerGroup,
    usb::usb_serial_jtag::UsbSerialJtag,
};
use esp_println::println;
use esp_radio::ble::controller::BleConnector;
use esp_radio::wifi::{
    AuthenticationMethod, AuthenticationMethodConfig, Config as WifiNetConfig, ControllerConfig,
    Password, Ssid, WifiController, scan::ScanConfig, sta::StationConfig,
};
use passport_core::api::App;
use passport_core::board::KeyState;
use passport_core::board::{
    FRAME_TICK_MS, I2C_CW2017_ADDR, I2C_ES8311_ADDR, I2S_RX_LOOPBACK_TX, INPUT_TICK_MS, OS_NAME,
    PIN_USB_DM, PIN_USB_DP, battery_poll_due, decode_millivolts, idle_telemetry_due,
};
use passport_core::boot::{BOOT_TICK_MS, BootAnim};
use passport_core::charging_from_samples;
use passport_core::console::parse_line;
use passport_core::flap::{Redraw, cadence_redraw};
use passport_core::paint::FrameSig;
use passport_core::radio::Resource;
use passport_core::shell::{EventOutcome, Overlay, Shell, SideEffect};
use passport_core::wifi::WifiNet;
use passport_core::{pcm16_le_level, pcm16_le_peak};
use trouble_host::prelude::*;

use crate::apps::Apps;
use crate::st7789::St7789;
use crate::ui::{COL_BG, overlay_name, paint, paint_boot};

esp_bootloader_esp_idf::esp_app_desc!();

const HEAP: usize = 72 * 1024;

enum WifiCmd {
    Scan,
    Connect {
        ssid: heapless::String<32>,
        pass: heapless::String<64>,
        open: bool,
    },
    Disconnect,
}

enum WifiEvt {
    ScanDone(heapless::Vec<WifiNet, 16>),
    Connected,
    Failed,
}

static WIFI_CMD: Channel<CriticalSectionRawMutex, WifiCmd, 2> = Channel::new();
static WIFI_EVT: Channel<CriticalSectionRawMutex, WifiEvt, 2> = Channel::new();

const SINE: [i16; 32] = [
    0, 6392, 12539, 18204, 23169, 27244, 30272, 32137, 32767, 32137, 30272, 27244, 23169, 18204,
    12539, 6392, 0, -6392, -12539, -18204, -23169, -27244, -30272, -32137, -32767, -32137, -30272,
    -27244, -23169, -18204, -12539, -6392,
];

struct NsDelay(Delay);

impl DelayNs for NsDelay {
    fn delay_ns(&mut self, ns: u32) {
        self.0.delay_nanos(ns);
    }
}

/// ESP32-C3 USB_SERIAL_JTAG_FRAM_NUM_REG (SOF index). Changes ~1 ms with a host.
fn usb_sof_frame() -> u16 {
    const FRAM_NUM: usize = 0x6004_3024;
    unsafe { (core::ptr::read_volatile(FRAM_NUM as *const u32) & 0x7FF) as u16 }
}

fn fill_silence(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        *b = 0;
    }
}

fn fill_pcm(buf: &mut [u8], idx: &mut usize) {
    let data = unsafe { core::slice::from_raw_parts(SINE.as_ptr() as *const u8, SINE.len() * 2) };
    for b in buf.iter_mut() {
        *b = data[*idx];
        *idx += 1;
        if *idx >= data.len() {
            *idx = 0;
        }
    }
}

#[esp_hal::main]
async fn main(spawner: Spawner) {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_alloc::heap_allocator!(size: 72 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    println!("[boot] {OS_NAME} r1");
    println!("[boot] cpu=160MHz heap={HEAP} no-psram");
    println!("[boot] usb Serial/JTAG GPIO{PIN_USB_DM}/{PIN_USB_DP}");

    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(40))
            .with_mode(Mode::_0),
    )
    .expect("spi")
    .with_sck(peripherals.GPIO8)
    .with_mosi(peripherals.GPIO9);
    let dc = Output::new(peripherals.GPIO20, Level::Low, OutputConfig::default());
    let cs = Output::new(peripherals.GPIO1, Level::High, OutputConfig::default());
    let mut lcd = St7789::new(spi, dc, cs);

    // GPIO21 often floats high → panel backlight on during DISPON of empty GRAM.
    let mut ledc = Ledc::new(peripherals.LEDC);
    ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);
    let mut lstimer0 = ledc.timer::<LowSpeed>(timer::Number::Timer0);
    let _ = lstimer0.configure(timer::config::Config {
        duty: timer::config::Duty::Duty10Bit,
        clock_source: timer::LSClockSource::APBClk,
        frequency: Rate::from_khz(5),
    });
    let mut bl = ledc.channel::<LowSpeed>(channel::Number::Channel0, peripherals.GPIO21);
    let _ = bl.configure(channel::config::Config {
        timer: &lstimer0,
        duty_pct: 0,
        drive_mode: DriveMode::PushPull,
    });
    let _ = bl.set_duty(0);

    let mut delay = NsDelay(Delay::new());
    match lcd.init(&mut delay, COL_BG) {
        Ok(()) => println!("[boot] display ST7789P3 240x320 invert-on"),
        Err(_) => println!("[boot] display init failed"),
    }

    // Keys + battery must be live before the first visible frame. I2S/USB
    // init after that — a blocking USB write used to stall the loop forever
    // when no host was listening, so the panel showed a dead first paint.
    let mut i2c = I2c::new(peripherals.I2C0, I2cConfig::default())
        .expect("i2c")
        .with_sda(peripherals.GPIO10)
        .with_scl(peripherals.GPIO7);
    let es_ok = codec::probe(&mut i2c, I2C_ES8311_ADDR);
    let cw_ok = codec::probe(&mut i2c, I2C_CW2017_ADDR);
    println!(
        "[boot] i2c probe es8311@0x18={} cw2017@0x63={}",
        if es_ok { "ok" } else { "miss" },
        if cw_ok { "ok" } else { "miss" }
    );
    if es_ok {
        let _ = codec::es8311_init(&mut i2c);
    }
    if cw_ok {
        let _ = codec::cw2017_wake(&mut i2c);
    }

    let mut adc_cfg = AdcConfig::new();
    let mut btn_pin =
        adc_cfg.enable_pin_with_cal::<_, AdcCalCurve<_>>(peripherals.GPIO0, Attenuation::_11dB);
    let mut adc = Adc::new(peripherals.ADC1, adc_cfg);

    let mut shell = Shell::new();
    let mut apps = Apps::new();
    let _ = shell.register_app(apps.pulse.id(), apps.pulse.name());
    let _ = shell.register_app(apps.nfc.id(), apps.nfc.name());
    let _ = shell.register_app(apps.flap.id(), apps.flap.name());
    let _ = shell.register_app(apps.stack.id(), apps.stack.name());
    let _ = shell.register_app(apps.brick.id(), apps.brick.name());
    let _ = shell.register_app(apps.boo.id(), apps.boo.name());
    let _ = shell.register_app(apps.tune.id(), apps.tune.name());
    shell.set_wants_mic(apps.boo.id(), true);
    shell.set_wants_mic(apps.tune.id(), true);
    let mut kv = crate::store::KvStore::open(peripherals.FLASH);
    if let Some(n) = kv.factory_image_bytes() {
        shell.set_factory_used(n);
        println!("[boot] factory image {n} bytes");
    }
    if let Some(mins) = passport_core::read_tod(&kv) {
        let _ = shell.status.clock.set_minutes(mins);
        println!("[clock] restore {}", shell.status.clock.format_hm());
    }
    let mut clock_anchor = Instant::now();
    let mut clock_applied: u64 = 0;
    shell.status.brightness = 100;
    if cw_ok {
        if let Some((soc, mv)) = codec::cw2017_soc_mv(&mut i2c) {
            shell.status.battery_soc = Some(soc);
            shell.status.battery_mv = Some(mv);
        }
    }
    let _ = bl.set_duty(100);
    println!("[boot] backlight PWM GPIO21 duty=100");
    {
        let mut mark = BootAnim::new();
        println!("[boot] mark");
        loop {
            let start = Instant::now();
            let _ = paint_boot(&mut lcd, &mark);
            mark.mark_painted();
            if mark.is_done() {
                break;
            }
            let mut sample = None;
            for _ in 0..32 {
                match adc.read_oneshot(&mut btn_pin) {
                    Ok(mv) => {
                        sample = Some(mv);
                        break;
                    }
                    Err(nb::Error::WouldBlock) => {}
                    Err(_) => break,
                }
            }
            if let Some(mv) = sample {
                if !matches!(decode_millivolts(mv), KeyState::Released) {
                    mark.skip();
                    continue;
                }
            }
            let used = start.elapsed().as_millis();
            let remain = u64::from(BOOT_TICK_MS).saturating_sub(used);
            if remain > 0 {
                Timer::after(Duration::from_millis(remain)).await;
            }
            mark.tick();
        }
    }

    shell.enter_home();
    println!("[shell] ready workspaces=2 overlay=launcher");
    println!("{}", shell.status.format());
    let mut frame: Option<FrameSig> = None;
    let _ = paint(&mut lcd, &shell, &apps, &mut frame);
    let mut last_overlay = shell.overlay();
    println!("[ui] painted overlay={}", overlay_name(last_overlay));

    // I2S0 full-duplex (MCLK6 BCLK5 WS3 DOUT2 DIN4) — streamed PCM, no 96 KB capture buf.
    let i2s = I2s::new(
        peripherals.I2S0,
        peripherals.DMA_CH0,
        TdmConfig::new_tdm_philips()
            .with_sample_rate(Rate::from_hz(16_000))
            .with_data_format(DataFormat::Data16Channel16)
            .with_channels(Channels::STEREO)
            .with_signal_loopback(I2S_RX_LOOPBACK_TX),
    )
    .expect("i2s")
    .with_mclk(peripherals.GPIO6)
    .into_async();
    let mut audio_tx = Some(
        i2s.i2s_tx
            .with_bclk(peripherals.GPIO5)
            .with_ws(peripherals.GPIO3)
            .with_dout(peripherals.GPIO2)
            .build(),
    );
    let mut audio_rx = Some(i2s.i2s_rx.with_din(peripherals.GPIO4).build());
    let mut tx_xfer = None;
    let mut rx_xfer = None;
    let mut sine_idx = 0usize;
    println!("[boot] audio worker started");

    let (mut usb_rx, mut usb_tx) = UsbSerialJtag::new(peripherals.USB_DEVICE)
        .into_async()
        .split();
    println!("[boot] usb console ready");
    println!("[boot] {OS_NAME} r1");

    let mut lp = LowPower::new(peripherals.LPWR);
    let mut wifi = Some(peripherals.WIFI);
    let mut bt = Some(peripherals.BT);

    let mut line = heapless::String::<128>::new();
    let mut byte = [0u8; 1];
    let mut ticks: u32 = 0;
    let mut last_usb_sof = usb_sof_frame();
    let mut last_soc = shell.status.battery_soc;
    let mut last_input = Instant::now();
    let mut last_frame = Instant::now();
    let mut mic_owns_i2s = false;
    let mut mic_logged = false;
    let mut mic_skip = 0u8;

    // Expands in `main` so button ticks and USB commands share one hardware path.
    macro_rules! apply_hw_side {
        ($side:expr) => {{
            match $side {
                SideEffect::None => {}
                SideEffect::SetBrightness(n) => {
                    let _ = bl.set_duty(n);
                    println!("[boot] display backlight {n}");
                }
                SideEffect::Probe => {
                    let p = codec::i2c_scan_line(&mut i2c);
                    println!("{p}");
                    let _ = usb_tx.write_all(p.as_bytes()).await;
                    let _ = usb_tx.write_all(b"\r\n").await;
                }
                SideEffect::AudioBeep => {
                    if tx_xfer.is_none() {
                        if let Some(tx) = audio_tx.take() {
                            let mut buf = dma_tx_stream_buffer!(4096, 1024);
                            buf.push_with(|b| {
                                fill_pcm(b, &mut sine_idx);
                                b.len()
                            });
                            match tx.write(buf) {
                                Ok(xfer) => {
                                    tx_xfer = Some(xfer);
                                    println!("[audio] playback started");
                                    let _ = usb_tx.write_all(b"[audio] playback started\r\n").await;
                                }
                                Err((_, tx, _)) => {
                                    audio_tx = Some(tx);
                                    println!("[audio] playback failed");
                                }
                            }
                        }
                    }
                }
                SideEffect::AudioRec => {
                    if rx_xfer.is_none() {
                        if let Some(rx) = audio_rx.take() {
                            let buf = dma_rx_stream_buffer!(4096, 1024);
                            match rx.read(buf) {
                                Ok(xfer) => {
                                    rx_xfer = Some(xfer);
                                    println!("[audio] record started");
                                    let _ = usb_tx.write_all(b"[audio] record started\r\n").await;
                                }
                                Err((_, rx, _)) => {
                                    audio_rx = Some(rx);
                                    println!("[audio] record failed");
                                }
                            }
                        }
                    }
                    // I2S master clocks come from TX; start a sine stream if idle.
                    if tx_xfer.is_none() {
                        if let Some(tx) = audio_tx.take() {
                            let mut buf = dma_tx_stream_buffer!(4096, 1024);
                            buf.push_with(|b| {
                                fill_pcm(b, &mut sine_idx);
                                b.len()
                            });
                            match tx.write(buf) {
                                Ok(xfer) => tx_xfer = Some(xfer),
                                Err((_, tx, _)) => audio_tx = Some(tx),
                            }
                        }
                    }
                }
                SideEffect::WifiScan => {
                    // Never await radio on this task — keys/paint live here.
                    if let Some(w) = wifi.take() {
                        match wifi_worker(w) {
                            Ok(token) => {
                                spawner.spawn(token);
                                println!("[wifi] worker");
                            }
                            Err(_) => println!("[wifi] spawn failed"),
                        }
                    }
                    if WIFI_CMD.try_send(WifiCmd::Scan).is_err() {
                        println!("[wifi] busy");
                    } else {
                        println!("[wifi] scanning");
                        let _ = usb_tx.write_all(b"[wifi] scanning\r\n").await;
                    }
                }
                SideEffect::WifiConnect => {
                    let mut ssid = heapless::String::<32>::new();
                    let mut pass = heapless::String::<64>::new();
                    let _ = ssid.push_str(shell.wifi_connect_ssid());
                    let _ = pass.push_str(shell.wifi_connect_pass());
                    let open = shell.wifi_connect_open();
                    if let Some(w) = wifi.take() {
                        match wifi_worker(w) {
                            Ok(token) => {
                                spawner.spawn(token);
                                println!("[wifi] worker");
                            }
                            Err(_) => println!("[wifi] spawn failed"),
                        }
                    }
                    if WIFI_CMD
                        .try_send(WifiCmd::Connect { ssid, pass, open })
                        .is_err()
                    {
                        println!("[wifi] busy");
                        shell.apply_wifi_result(false);
                    } else {
                        println!("[wifi] join");
                    }
                }
                SideEffect::BleAdvertise => {
                    if let Some(b) = bt.take() {
                        match ble_advertise(b) {
                            Ok(token) => {
                                spawner.spawn(token);
                                println!("[ble] advertising name=PassportOS");
                                let _ = usb_tx
                                    .write_all(b"[ble] advertising name=PassportOS\r\n")
                                    .await;
                            }
                            Err(_) => println!("[ble] spawn failed"),
                        }
                    } else {
                        println!("[ble] busy");
                    }
                }
                SideEffect::RadioOff => {
                    let _ = WIFI_CMD.try_send(WifiCmd::Disconnect);
                    println!("[radio] off");
                }
                SideEffect::ClockSet => {
                    if let Some(mins) = shell.status.clock.minutes_of_day() {
                        passport_core::write_tod(&mut kv, mins);
                    }
                    clock_anchor = Instant::now();
                    clock_applied = 0;
                    println!("[clock] {}", shell.status.clock.format_hm());
                }
                SideEffect::SleepLight => {
                    println!("[sleep] light 2000ms");
                    let _ = usb_tx.write_all(b"[sleep] light 2000ms\r\n").await;
                    lp.set_wakeup_deadline(
                        esp_hal::time::Instant::now() + esp_hal::time::Duration::from_millis(2000),
                    );
                    lp.sleep_light(RtcSleepConfig::default());
                    println!("[sleep] wake rtc");
                }
                SideEffect::SleepDeep => {
                    println!("[sleep] deep 5000ms");
                    let _ = usb_tx.write_all(b"[sleep] deep 5000ms\r\n").await;
                    Timer::after(Duration::from_millis(30)).await;
                    lp.set_wakeup_deadline(
                        esp_hal::time::Instant::now() + esp_hal::time::Duration::from_secs(5),
                    );
                    lp.sleep_deep(RtcSleepConfig::default());
                }
            }
        }};
    }

    macro_rules! read_ladder {
        () => {{
            let mut sample = None;
            for _ in 0..32 {
                match adc.read_oneshot(&mut btn_pin) {
                    Ok(mv) => {
                        sample = Some(mv);
                        break;
                    }
                    Err(nb::Error::WouldBlock) => {}
                    Err(_) => break,
                }
            }
            sample
        }};
    }

    // No `[btn]` USB print here: Serial/JTAG writes on the 5 ms sample path
    // were the stall in on-device logs (`[btn] mv=304 down` / `rel` chatter).
    macro_rules! handle_keys {
        ($mv:expr, $dt:expr, $tick_flap:expr, $mic_notes:expr, $mic_lv:expr, $mic_hz:expr) => {{
            let mv = $mv;
            let mut ev = shell.tick_mv(mv, $dt);
            for n in $mic_notes {
                let _ = ev.lifecycle.push(*n);
            }
            crate::apps::pulse::dispatch(&mut apps.pulse, &ev.lifecycle, mv);
            crate::apps::nfc::dispatch(&mut apps.nfc, &ev.lifecycle, mv);
            let desk = shell.overlay() == Overlay::None;
            let focused = shell.focused_app_name();
            let live = $tick_flap && desk && !shell.is_standby();
            crate::apps::flap::dispatch(
                &mut apps.flap,
                &ev.lifecycle,
                mv,
                live && focused == Some("flap"),
                &mut kv,
            );
            crate::apps::stack::dispatch(
                &mut apps.stack,
                &ev.lifecycle,
                mv,
                live && focused == Some("stack"),
                &mut kv,
            );
            crate::apps::brick::dispatch(
                &mut apps.brick,
                &ev.lifecycle,
                mv,
                live && focused == Some("brick"),
                &mut kv,
            );
            crate::apps::boo::dispatch(
                &mut apps.boo,
                &ev.lifecycle,
                mv,
                live && focused == Some("boo"),
                &mut kv,
                $mic_lv,
            );
            crate::apps::tune::dispatch(
                &mut apps.tune,
                &ev.lifecycle,
                mv,
                live && focused == Some("tune"),
                &mut kv,
                $mic_lv,
                $mic_hz,
                Some(shell.pitch_buf()),
            );
            if ev.side != SideEffect::None {
                apply_hw_side!(ev.side);
            }
        }};
    }

    macro_rules! request_game {
        ($frame_due:expr) => {
            if !shell.is_standby() && shell.overlay() == Overlay::None {
                let want = match shell.focused_app_name() {
                    Some("flap") => apps.flap.redraw(),
                    Some("stack") => apps.stack.redraw(),
                    Some("brick") => apps.brick.redraw(),
                    Some("boo") => apps.boo.redraw(),
                    Some("tune") => apps.tune.redraw(),
                    _ => Redraw::None,
                };
                match cadence_redraw(want, $frame_due) {
                    Redraw::Full => shell.request_full(),
                    Redraw::Live => shell.request_live(),
                    Redraw::None => {}
                }
            }
        };
    }

    loop {
        let start = Instant::now();
        let elapsed = clock_anchor.elapsed().as_secs();
        if elapsed > clock_applied {
            let step = (elapsed - clock_applied) as u32;
            clock_applied = elapsed;
            let _ = shell.advance_clock(step);
        }
        let dt = last_input.elapsed().as_millis().clamp(1, 50) as u32;
        last_input = start;
        let frame_due = last_frame.elapsed().as_millis() >= u64::from(FRAME_TICK_MS);
        if let Ok(evt) = WIFI_EVT.try_receive() {
            match evt {
                WifiEvt::ScanDone(nets) => {
                    let n = nets.len();
                    shell.apply_wifi_scan(&nets);
                    println!("[wifi] scan count={n}");
                }
                WifiEvt::Connected => {
                    shell.apply_wifi_result(true);
                    println!("[wifi] joined {}", shell.wifi().connected_ssid());
                }
                WifiEvt::Failed => {
                    shell.apply_wifi_result(false);
                    println!("[wifi] join fail");
                }
            }
        }

        if frame_due {
            last_frame = start;
            ticks = ticks.wrapping_add(1);
            let sof = usb_sof_frame();
            let charging =
                charging_from_samples(sof, last_usb_sof, shell.status.battery_soc, last_soc);
            last_usb_sof = sof;
            let chg = shell.set_charging(charging);
            if chg.side != SideEffect::None {
                apply_hw_side!(chg.side);
            }
        }

        let want_mic = shell.mic_listen();
        let mut mic_level = None;
        if want_mic {
            if !mic_owns_i2s {
                shell.mic_begin();
                let prev = shell.exclusive.acquire(Resource::Audio);
                if matches!(prev, Some(Resource::Wifi | Resource::Ble)) {
                    apply_hw_side!(SideEffect::RadioOff);
                    shell.status.radio = passport_core::RadioMode::Off;
                }
                // TX clocks the codec; RX slaves via sig_loopback. Start both
                // DMA before poking the ADC so BCLK/WS already exist.
                if tx_xfer.is_none() {
                    if let Some(tx) = audio_tx.take() {
                        let mut buf = dma_tx_stream_buffer!(4096, 1024);
                        buf.push_with(|b| {
                            fill_silence(b);
                            b.len()
                        });
                        match tx.write(buf) {
                            Ok(xfer) => tx_xfer = Some(xfer),
                            Err((_, tx, _)) => audio_tx = Some(tx),
                        }
                    }
                }
                if rx_xfer.is_none() {
                    if let Some(rx) = audio_rx.take() {
                        let buf = dma_rx_stream_buffer!(4096, 1024);
                        match rx.read(buf) {
                            Ok(xfer) => rx_xfer = Some(xfer),
                            Err((_, rx, _)) => audio_rx = Some(rx),
                        }
                    }
                }
                Timer::after(Duration::from_millis(20)).await;
                if codec::es8311_start(&mut i2c).is_err() {
                    println!("[mic] codec start fail");
                } else {
                    let r01 = codec::es8311_read(&mut i2c, 0x01).unwrap_or(0);
                    let r14 = codec::es8311_read(&mut i2c, 0x14).unwrap_or(0);
                    let r17 = codec::es8311_read(&mut i2c, 0x17).unwrap_or(0);
                    println!("[mic] codec 01={r01:02x} 14={r14:02x} 17={r17:02x}");
                }
                mic_owns_i2s = true;
                mic_logged = false;
                mic_skip = 4;
                println!("[mic] listen");
            }
            if let Some(xfer) = rx_xfer.as_mut() {
                let mut tmp = [0u8; 256];
                let n = xfer.pop(&mut tmp);
                if n > 0 {
                    shell.feed_pcm(&tmp[..n]);
                }
                if n > 0 && mic_skip > 0 {
                    mic_skip -= 1;
                } else if n > 0 {
                    let lv = pcm16_le_level(&tmp[..n]);
                    let pk = pcm16_le_peak(&tmp[..n]);
                    mic_level = Some(lv);
                    shell.note_mic_peak(pk);
                    if !mic_logged {
                        println!(
                            "[mic] lv={lv} peak={pk} n={n} {:02x}{:02x} {:02x}{:02x} {:02x}{:02x} {:02x}{:02x}",
                            tmp.get(0).copied().unwrap_or(0),
                            tmp.get(1).copied().unwrap_or(0),
                            tmp.get(2).copied().unwrap_or(0),
                            tmp.get(3).copied().unwrap_or(0),
                            tmp.get(4).copied().unwrap_or(0),
                            tmp.get(5).copied().unwrap_or(0),
                            tmp.get(6).copied().unwrap_or(0),
                            tmp.get(7).copied().unwrap_or(0),
                        );
                        mic_logged = true;
                    }
                }
            }
        } else if mic_owns_i2s {
            if let Some(xfer) = rx_xfer.take() {
                let (rx, _) = xfer.stop();
                audio_rx = Some(rx);
            }
            if let Some(xfer) = tx_xfer.take() {
                let (tx, _) = xfer.stop();
                audio_tx = Some(tx);
            }
            shell.exclusive.release(Resource::Audio);
            mic_owns_i2s = false;
        }

        let mic_out = if let Some(lv) = mic_level {
            shell.tick_mic(lv)
        } else {
            EventOutcome::empty()
        };
        let mic_lv = mic_level.unwrap_or(0);

        let mut got = false;
        if let Some(mv) = read_ladder!() {
            handle_keys!(
                mv,
                dt,
                frame_due,
                &mic_out.lifecycle,
                mic_lv,
                shell.mic_hz()
            );
            got = true;
        } else {
            Timer::after(Duration::from_millis(1)).await;
            if let Some(mv) = read_ladder!() {
                handle_keys!(
                    mv,
                    dt.saturating_add(1),
                    frame_due,
                    &mic_out.lifecycle,
                    mic_lv,
                    shell.mic_hz()
                );
                got = true;
            }
        }
        if frame_due && !got && !shell.is_standby() && shell.overlay() == Overlay::None {
            match shell.focused_app_name() {
                Some("flap") => crate::apps::flap::dispatch(&mut apps.flap, &[], 0, true, &mut kv),
                Some("stack") => {
                    crate::apps::stack::dispatch(&mut apps.stack, &[], 0, true, &mut kv)
                }
                Some("brick") => {
                    crate::apps::brick::dispatch(&mut apps.brick, &[], 0, true, &mut kv)
                }
                Some("boo") => crate::apps::boo::dispatch(
                    &mut apps.boo,
                    &mic_out.lifecycle,
                    0,
                    true,
                    &mut kv,
                    mic_lv,
                ),
                Some("tune") => crate::apps::tune::dispatch(
                    &mut apps.tune,
                    &[],
                    0,
                    true,
                    &mut kv,
                    mic_lv,
                    shell.mic_hz(),
                    Some(shell.pitch_buf()),
                ),
                _ => {}
            }
        } else if !got && !mic_out.lifecycle.is_empty() {
            crate::apps::boo::dispatch(
                &mut apps.boo,
                &mic_out.lifecycle,
                0,
                false,
                &mut kv,
                mic_lv,
            );
        }
        request_game!(frame_due);

        if let Some(xfer) = tx_xfer.as_mut() {
            if xfer.available_bytes() > 0 {
                let _ = xfer.push_with(|buf| {
                    if mic_owns_i2s {
                        fill_silence(buf);
                    } else {
                        fill_pcm(buf, &mut sine_idx);
                    }
                    buf.len()
                });
            }
        }
        if !mic_owns_i2s {
            if let Some(xfer) = rx_xfer.as_mut() {
                let mut tmp = [0u8; 256];
                let n = xfer.pop(&mut tmp);
                if n > 0 {
                    println!("[audio] record {n} bytes");
                }
            }
        }

        if shell.dirty && !shell.is_standby() {
            let mark_flap = shell.overlay() == Overlay::None
                && shell.focused_app_name() == Some("flap")
                && apps.flap.redraw() != Redraw::None;
            let mark_stack = shell.overlay() == Overlay::None
                && shell.focused_app_name() == Some("stack")
                && apps.stack.redraw() != Redraw::None;
            let mark_brick = shell.overlay() == Overlay::None
                && shell.focused_app_name() == Some("brick")
                && apps.brick.redraw() != Redraw::None;
            let mark_boo = shell.overlay() == Overlay::None
                && shell.focused_app_name() == Some("boo")
                && apps.boo.redraw() != Redraw::None;
            let mark_tune = shell.overlay() == Overlay::None
                && shell.focused_app_name() == Some("tune")
                && apps.tune.redraw() != Redraw::None;
            shell.dirty = false;
            if paint(&mut lcd, &shell, &apps, &mut frame).is_err() {
                println!("[boot] display paint failed");
            } else {
                if mark_flap {
                    apps.flap.mark_painted();
                }
                if mark_stack {
                    apps.stack.mark_painted();
                }
                if mark_brick {
                    apps.brick.mark_painted();
                }
                if mark_boo {
                    apps.boo.mark_painted();
                }
                if mark_tune {
                    apps.tune.mark_painted();
                }
                if shell.overlay() != last_overlay {
                    last_overlay = shell.overlay();
                    println!("[ui] painted overlay={}", overlay_name(last_overlay));
                }
            }
        }

        // Catch the edge that happened while SPI owned the bus.
        let dt2 = last_input.elapsed().as_millis().clamp(1, 50) as u32;
        last_input = Instant::now();
        if let Some(mv) = read_ladder!() {
            handle_keys!(mv, dt2, false, &[], mic_lv, shell.mic_hz());
            request_game!(false);
        }

        if frame_due && battery_poll_due(ticks) {
            if cw_ok {
                if let Some((soc, mv)) = codec::cw2017_soc_mv(&mut i2c) {
                    last_soc = shell.status.battery_soc;
                    if shell.status.battery_soc != Some(soc) {
                        shell.status.battery_soc = Some(soc);
                        shell.status.battery_mv = Some(mv);
                        shell.dirty = true;
                    }
                }
            }
            shell.refresh_status();
            if idle_telemetry_due(ticks) {
                println!("{}", shell.status.format());
            }
        }

        let used = start.elapsed().as_millis();
        let remain = u64::from(INPUT_TICK_MS).saturating_sub(used);
        match select(
            usb_rx.read(&mut byte),
            Timer::after(Duration::from_millis(remain)),
        )
        .await
        {
            Either::First(Ok(1)) => match byte[0] {
                b'\r' | b'\n' => {
                    if !line.is_empty() {
                        let parsed = parse_line(line.as_str());
                        line.clear();
                        match parsed {
                            Ok(c) => {
                                let out = shell.apply_command(c);
                                apply_hw_side!(out.side);
                                let mut reply = heapless::String::<192>::new();
                                let _ = write!(reply, "{}\r\n", out.reply.as_str());
                                let _ = usb_tx.write_all(reply.as_bytes()).await;
                                println!("{}", out.reply.as_str());
                                let mv = apps.pulse.last_mv();
                                crate::apps::pulse::dispatch(&mut apps.pulse, &out.lifecycle, mv);
                                crate::apps::nfc::dispatch(&mut apps.nfc, &out.lifecycle, mv);
                                crate::apps::flap::dispatch(
                                    &mut apps.flap,
                                    &out.lifecycle,
                                    mv,
                                    false,
                                    &mut kv,
                                );
                                crate::apps::stack::dispatch(
                                    &mut apps.stack,
                                    &out.lifecycle,
                                    mv,
                                    false,
                                    &mut kv,
                                );
                                crate::apps::brick::dispatch(
                                    &mut apps.brick,
                                    &out.lifecycle,
                                    mv,
                                    false,
                                    &mut kv,
                                );
                                crate::apps::boo::dispatch(
                                    &mut apps.boo,
                                    &out.lifecycle,
                                    mv,
                                    false,
                                    &mut kv,
                                    0,
                                );
                                crate::apps::tune::dispatch(
                                    &mut apps.tune,
                                    &out.lifecycle,
                                    mv,
                                    false,
                                    &mut kv,
                                    0,
                                    0,
                                    None,
                                );
                            }
                            Err(_) => {
                                let _ = usb_tx.write_all(b"?\r\n").await;
                            }
                        }
                    }
                }
                b'\x08' | b'\x7f' => {
                    let _ = line.pop();
                }
                c if c.is_ascii() && line.len() + 1 < line.capacity() => {
                    let _ = line.push(c as char);
                }
                _ => {}
            },
            _ => {}
        }
    }
}

fn wifi_net_from_ap(ssid: &str, open: bool, rssi: i8) -> Option<WifiNet> {
    WifiNet::new(ssid, open, rssi).ok()
}

#[embassy_executor::task]
async fn wifi_worker(wifi: esp_hal::peripherals::WIFI<'static>) {
    // Yield so the UI loop keeps reading keys if radio init is slow.
    Timer::after(Duration::from_millis(1)).await;
    println!("[wifi] starting");
    let mut controller = match WifiController::new(
        wifi,
        ControllerConfig::default()
            .with_initial_config(WifiNetConfig::Station(StationConfig::default())),
    ) {
        Ok(c) => c,
        Err(_) => {
            println!("[wifi] controller failed");
            return;
        }
    };
    loop {
        match WIFI_CMD.receive().await {
            WifiCmd::Scan => {
                let scan_config = ScanConfig::default().with_max(16);
                let mut nets = heapless::Vec::<WifiNet, 16>::new();
                match select(
                    controller.scan_async(&scan_config),
                    Timer::after(Duration::from_secs(8)),
                )
                .await
                {
                    Either::First(Ok(result)) => {
                        for ap in result {
                            let ssid = ap.ssid.as_str();
                            if ssid.is_empty() {
                                continue;
                            }
                            let open = matches!(
                                ap.auth_method,
                                None | Some(AuthenticationMethod::None)
                                    | Some(AuthenticationMethod::Owe)
                            );
                            if let Some(n) = wifi_net_from_ap(ssid, open, ap.signal_strength) {
                                let _ = nets.push(n);
                            }
                        }
                    }
                    Either::First(Err(_)) => println!("[wifi] scan err"),
                    Either::Second(_) => println!("[wifi] scan timeout"),
                }
                WIFI_EVT.send(WifiEvt::ScanDone(nets)).await;
            }
            WifiCmd::Connect { ssid, pass, open } => {
                let auth = if open {
                    AuthenticationMethodConfig::Open
                } else {
                    match Password::try_from(pass.as_str()) {
                        Ok(p) => AuthenticationMethodConfig::WpaWpa2Personal(p),
                        Err(_) => {
                            WIFI_EVT.send(WifiEvt::Failed).await;
                            continue;
                        }
                    }
                };
                let Ok(ssid) = Ssid::try_from(ssid.as_str()) else {
                    WIFI_EVT.send(WifiEvt::Failed).await;
                    continue;
                };
                let cfg = StationConfig::default()
                    .with_ssid(ssid)
                    .with_authentication(auth);
                if controller.set_config(&WifiNetConfig::Station(cfg)).is_err() {
                    println!("[wifi] config failed");
                    WIFI_EVT.send(WifiEvt::Failed).await;
                    continue;
                }
                match select(
                    controller.connect_async(),
                    Timer::after(Duration::from_secs(15)),
                )
                .await
                {
                    Either::First(Ok(_)) => WIFI_EVT.send(WifiEvt::Connected).await,
                    Either::First(Err(_)) => {
                        println!("[wifi] join err");
                        WIFI_EVT.send(WifiEvt::Failed).await;
                    }
                    Either::Second(_) => {
                        println!("[wifi] join timeout");
                        WIFI_EVT.send(WifiEvt::Failed).await;
                    }
                }
            }
            WifiCmd::Disconnect => {
                let _ = controller.disconnect_async().await;
                println!("[wifi] off");
            }
        }
    }
}

#[embassy_executor::task]
async fn ble_advertise(bt: esp_hal::peripherals::BT<'static>) {
    Timer::after(Duration::from_millis(1)).await;
    println!("[ble] starting");
    let connector = match BleConnector::new(bt, Default::default()) {
        Ok(c) => c,
        Err(_) => {
            println!("[ble] connector failed");
            return;
        }
    };
    let controller: ExternalController<_, 1> = ExternalController::new(connector);
    let address = Address::random([0x50, 0x41, 0x53, 0x53, 0x4F, 0x53]);
    let mut resources: HostResources<_, DefaultPacketPool, 1, 2> = HostResources::new();
    let stack = trouble_host::new(controller, &mut resources)
        .set_random_address(address)
        .build();
    let mut peripheral = stack.peripheral();
    let mut runner = stack.runner();
    let mut adv_data = [0u8; 31];
    let len = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::CompleteLocalName(b"PassportOS"),
        ],
        &mut adv_data,
    )
    .unwrap_or(0);
    println!("[ble] advertising name=PassportOS");
    let _ = embassy_futures::join::join(
        async {
            let _ = runner.run().await;
        },
        async {
            let params = Default::default();
            let _ = peripheral
                .advertise(
                    &params,
                    Advertisement::NonconnectableNonscannableUndirected {
                        adv_data: &adv_data[..len],
                    },
                )
                .await;
            loop {
                Timer::after(Duration::from_secs(2)).await;
            }
        },
    )
    .await;
}
