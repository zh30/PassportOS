//! Installed apps. Add a new file and a field on [`Apps`] — do not edit drivers.

pub mod boo;
pub mod brick;
pub mod flap;
mod keys;
pub mod nfc;
pub mod pulse;
pub mod stack;
mod system;
pub mod tune;

pub use boo::BooApp;
pub use brick::BrickApp;
pub use flap::FlapApp;
pub use nfc::NfcApp;
pub use pulse::PulseApp;
pub use stack::StackApp;
pub use tune::TuneApp;

use passport_core::AppId;
use passport_core::api::App;

pub struct Apps {
    pub pulse: PulseApp,
    pub nfc: NfcApp,
    pub flap: FlapApp,
    pub stack: StackApp,
    pub brick: BrickApp,
    pub boo: BooApp,
    pub tune: TuneApp,
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
            boo: BooApp::new(),
            tune: TuneApp::new(),
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
        } else if id == self.boo.id() {
            Some(&mut self.boo)
        } else if id == self.tune.id() {
            Some(&mut self.tune)
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
        f(&mut self.boo);
        f(&mut self.tune);
        f(&mut self.system);
        f(&mut self.keys);
    }
}
