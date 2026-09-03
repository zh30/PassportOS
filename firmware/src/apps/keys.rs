//! Built-in Shortcuts. Drawing still lives in `ui.rs` until Stage 2.5.

use passport_core::AppId;
use passport_core::api::{App, Cx};
use passport_core::compositor::Rect;

pub struct KeysApp;

impl KeysApp {
    pub const fn new() -> Self {
        Self
    }
}

impl App for KeysApp {
    fn id(&self) -> AppId {
        AppId(0xFD)
    }
    fn name(&self) -> &'static str {
        "keys"
    }
    fn title(&self) -> &'static str {
        "Shortcuts"
    }
    fn blurb(&self) -> &'static str {
        "button map"
    }
    fn draw(&self, _cx: &mut Cx<'_>, _viewport: Rect) {}
}
