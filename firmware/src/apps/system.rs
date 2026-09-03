//! Built-in Control center. Drawing still lives in `ui.rs` until Stage 2.5;
//! this type exists so launcher metadata and lifecycle go through [`App`].

use passport_core::AppId;
use passport_core::api::{App, Cx};
use passport_core::compositor::Rect;

pub struct SystemApp;

impl SystemApp {
    pub const fn new() -> Self {
        Self
    }
}

impl App for SystemApp {
    fn id(&self) -> AppId {
        AppId(0xFE)
    }
    fn name(&self) -> &'static str {
        "system"
    }
    fn title(&self) -> &'static str {
        "Control"
    }
    fn blurb(&self) -> &'static str {
        "wifi  ble  sleep"
    }
    fn draw(&self, _cx: &mut Cx<'_>, _viewport: Rect) {}
}
