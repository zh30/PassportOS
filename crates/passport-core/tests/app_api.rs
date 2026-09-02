//! Host tests for the unified App / Cx API. No HAL types.

use passport_core::api::{
    apply_notes, ApiError, App, Audio, Battery, ClippedDraw, Cx, Draw, ExclusiveAudio,
    ExclusiveRadio, MemoryStore, NullAudio, NullDraw, NullPower, NullRadio, Power, Radio, Store,
};
use passport_core::app::{AppId, AppLifecycle};
use passport_core::compositor::{layout_tiles, Rect, STATUS_BAR_H};
use passport_core::input::ButtonEvent;
use passport_core::radio::{ExclusiveManager, Resource};
use passport_core::board::Key;

const HELLO: AppId = AppId(7);

struct RecDraw {
    clip: Rect,
    fills: Vec<(Rect, u16)>,
    texts: Vec<(u16, u16, String)>,
}

impl RecDraw {
    fn new(clip: Rect) -> Self {
        Self {
            clip,
            fills: Vec::new(),
            texts: Vec::new(),
        }
    }
}

impl Draw for RecDraw {
    fn fill(&mut self, r: Rect, rgb565: u16) {
        self.fills.push((r, rgb565));
    }
    fn text(&mut self, x: u16, y: u16, s: &str, _fg: u16, _bg: u16) {
        self.texts.push((x, y, s.to_string()));
    }
    fn text_2x(&mut self, x: u16, y: u16, s: &str, fg: u16, bg: u16) {
        self.text(x, y, s, fg, bg);
    }
    fn clip(&self) -> Rect {
        self.clip
    }
}

struct HelloApp {
    started: bool,
    focused: bool,
    keys: Vec<ButtonEvent>,
    ticks: u32,
    last_adc: u16,
}

impl HelloApp {
    fn new() -> Self {
        Self {
            started: false,
            focused: false,
            keys: Vec::new(),
            ticks: 0,
            last_adc: 0,
        }
    }
}

impl App for HelloApp {
    fn id(&self) -> AppId {
        HELLO
    }
    fn name(&self) -> &'static str {
        "hello"
    }
    fn title(&self) -> &'static str {
        "Hello"
    }
    fn blurb(&self) -> &'static str {
        "api sample"
    }
    fn on_start(&mut self, _cx: &mut Cx<'_>) {
        self.started = true;
    }
    fn on_stop(&mut self, _cx: &mut Cx<'_>) {
        self.started = false;
        self.focused = false;
    }
    fn on_focus(&mut self, _cx: &mut Cx<'_>) {
        self.focused = true;
    }
    fn on_blur(&mut self, _cx: &mut Cx<'_>) {
        self.focused = false;
    }
    fn on_key(&mut self, _cx: &mut Cx<'_>, ev: ButtonEvent) {
        self.keys.push(ev);
    }
    fn on_tick(&mut self, cx: &mut Cx<'_>, dt_ms: u32) {
        self.ticks = self.ticks.saturating_add(dt_ms);
        self.last_adc = cx.adc_mv();
    }
    fn draw(&self, cx: &mut Cx<'_>, vp: Rect) {
        cx.draw.fill(vp, 0x10A2);
        cx.draw.text(vp.x + 4, vp.y + 4, "Hello", 0x07FD, 0x10A2);
        // Attempt to paint the status bar — clip must drop this.
        cx.draw.fill(
            Rect {
                x: 0,
                y: 0,
                w: 240,
                h: STATUS_BAR_H,
            },
            0xF800,
        );
    }
}

fn with_cx<R>(
    draw: &mut dyn Draw,
    audio: &mut dyn Audio,
    store: &mut dyn Store,
    radio: &mut dyn Radio,
    power: &mut dyn Power,
    adc_mv: u16,
    f: impl FnOnce(&mut Cx<'_>) -> R,
) -> R {
    let mut cx = Cx::new(
        draw,
        audio,
        store,
        radio,
        power,
        0,
        Battery::unknown(),
        100,
        adc_mv,
    );
    f(&mut cx)
}

#[test]
fn hello_app_lifecycle_and_clipped_draw() {
    let mut app = HelloApp::new();
    let tile = layout_tiles(1).tiles[0];
    let mut rec = RecDraw::new(tile);
    let mut audio = NullAudio;
    let mut store = MemoryStore::new();
    let mut radio = NullRadio;
    let mut power = NullPower::new(100);

    {
        let mut clipped = ClippedDraw::new(&mut rec, tile);
        with_cx(
            &mut clipped,
            &mut audio,
            &mut store,
            &mut radio,
            &mut power,
            3300,
            |cx| {
                apply_notes(
                    &mut app,
                    &[
                        AppLifecycle::Start(HELLO),
                        AppLifecycle::Focus(HELLO),
                        AppLifecycle::Input(HELLO, ButtonEvent::Click(Key::Down)),
                    ],
                    cx,
                );
                app.on_tick(cx, 20);
                app.draw(cx, tile);
            },
        );
    }

    assert!(app.started);
    assert!(app.focused);
    assert_eq!(app.keys, vec![ButtonEvent::Click(Key::Down)]);
    assert_eq!(app.ticks, 20);
    assert_eq!(app.last_adc, 3300);
    assert!(
        rec.fills.iter().any(|(r, c)| r.y >= STATUS_BAR_H && *c == 0x10A2),
        "app fill must land in the tile, got {:?}",
        rec.fills
    );
    assert!(
        rec.fills.iter().all(|(r, _)| r.y >= STATUS_BAR_H || r.intersection(tile).is_some() && r.y >= tile.y),
        "status bar must stay unpainted by the app: {:?}",
        rec.fills
    );
    assert!(
        !rec.fills.iter().any(|(r, c)| r.y < STATUS_BAR_H && *c == 0xF800 && r.h == STATUS_BAR_H),
        "clipped status-bar fill leaked: {:?}",
        rec.fills
    );
    assert_eq!(rec.texts[0].2, "Hello");
}

#[test]
fn audio_busy_when_radio_owns_sram() {
    let mut exclusive = ExclusiveManager::new();
    exclusive.acquire(Resource::Wifi);
    let mut audio = ExclusiveAudio::new(&mut exclusive);
    let err = audio.play_pcm16(16_000).unwrap_err();
    assert_eq!(
        err,
        ApiError::Busy {
            owner: Some(Resource::Wifi)
        }
    );
}

#[test]
fn radio_busy_when_audio_owns_sram() {
    let mut exclusive = ExclusiveManager::new();
    exclusive.acquire(Resource::Audio);
    let mut radio = ExclusiveRadio::new(&mut exclusive);
    let err = radio.wifi_scan().unwrap_err();
    assert_eq!(
        err,
        ApiError::Busy {
            owner: Some(Resource::Audio)
        }
    );
}

#[test]
fn memory_store_roundtrip_and_limits() {
    let mut store = MemoryStore::new();
    assert_eq!(store.get(b"k", &mut [0; 8]), None);
    store.put(b"k", b"hello").unwrap();
    let mut buf = [0u8; 8];
    assert_eq!(store.get(b"k", &mut buf), Some(5));
    assert_eq!(&buf[..5], b"hello");
    store.put(b"k", b"z").unwrap();
    assert_eq!(store.get(b"k", &mut buf), Some(1));
    assert_eq!(store.put(b"", b"x"), Err(ApiError::BadArg));
}

#[test]
fn tile_does_not_overlap_status_bar() {
    let layout = layout_tiles(1);
    assert_eq!(layout.status.h, STATUS_BAR_H);
    assert_eq!(layout.tiles[0].y, STATUS_BAR_H);
    assert!(layout.is_non_overlapping());
    let miss = Rect {
        x: 0,
        y: 0,
        w: 240,
        h: STATUS_BAR_H,
    };
    assert!(layout.tiles[0].intersection(miss).is_none());
}

#[test]
fn notes_for_other_id_are_ignored() {
    let mut app = HelloApp::new();
    let mut draw = NullDraw::new(layout_tiles(1).tiles[0]);
    let mut audio = NullAudio;
    let mut store = MemoryStore::new();
    let mut radio = NullRadio;
    let mut power = NullPower::new(80);
    with_cx(
        &mut draw,
        &mut audio,
        &mut store,
        &mut radio,
        &mut power,
        0,
        |cx| {
            apply_notes(&mut app, &[AppLifecycle::Start(AppId(1))], cx);
        },
    );
    assert!(!app.started);
}
