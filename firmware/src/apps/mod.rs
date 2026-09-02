//! Installed apps. Add a new file and a field on [`Apps`] — do not edit drivers.

pub mod brick;
pub mod flap;
pub mod stack;
mod keys;
pub mod nfc;
pub mod pulse;
mod system;

pub use flap::FlapApp;
pub use nfc::NfcApp;
pub use pulse::PulseApp;
pub use brick::BrickApp;
pub use stack::StackApp;

use passport_core::api::App;
use passport_core::AppId;

pub struct Apps {
    pub pulse: PulseApp,
    pub nfc: NfcApp,
    pub flap: FlapApp,
    pub stack: StackApp,
    pub brick: BrickApp,
    #[allow(dead_code)]
    pub system: system::SystemApp,
    #[allow(dead_code)]
    pub keys: keys::KeysApp,
}

impl Apps {
    pub fn new() -> Self {
        Self {
            pulse: PulseApp::new(),
            nfc: NfcApp::new(),
            flap: FlapApp::new(),
            stack: StackApp::new(),
            brick: BrickApp::new(),
            system: system::SystemApp::new(),
            keys: keys::KeysApp::new(),
        }
    }

    #[allow(dead_code)]
    pub fn get_mut(&mut self, id: AppId) -> Option<&mut dyn App> {
        if id == self.pulse.id() {
            Some(&mut self.pulse)
        } else if id == self.nfc.id() {
            Some(&mut self.nfc)
        } else if id == self.flap.id() {
            Some(&mut self.flap)
        } else if id == self.stack.id() {
            Some(&mut self.stack)
        } else if id == self.brick.id() {
            Some(&mut self.brick)
        } else if id == self.system.id() {
            Some(&mut self.system)
        } else if id == self.keys.id() {
            Some(&mut self.keys)
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub fn for_each_mut(&mut self, mut f: impl FnMut(&mut dyn App)) {
        f(&mut self.pulse);
        f(&mut self.nfc);
        f(&mut self.flap);
        f(&mut self.stack);
        f(&mut self.brick);
        f(&mut self.system);
        f(&mut self.keys);
    }
}
