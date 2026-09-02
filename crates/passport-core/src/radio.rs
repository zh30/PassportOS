//! Exclusive radio/audio ownership. Wi-Fi, BLE, and large audio DMA do not coexist.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    Wifi,
    Ble,
    Audio,
}

#[derive(Clone, Debug)]
pub struct ExclusiveManager {
    owner: Option<Resource>,
}

impl Default for ExclusiveManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ExclusiveManager {
    pub const fn new() -> Self {
        Self { owner: None }
    }

    pub fn owner(&self) -> Option<Resource> {
        self.owner
    }

    /// Acquire `want`. Returns the resource that was displaced, if any.
    pub fn acquire(&mut self, want: Resource) -> Option<Resource> {
        let prev = self.owner;
        self.owner = Some(want);
        if prev == Some(want) { None } else { prev }
    }

    pub fn release(&mut self, r: Resource) {
        if self.owner == Some(r) {
            self.owner = None;
        }
    }

    pub fn is_free(&self) -> bool {
        self.owner.is_none()
    }

    /// True if `want` may start without dropping another heavy resource.
    pub fn can_share_with(&self, want: Resource) -> bool {
        match (self.owner, want) {
            (None, _) => true,
            (Some(a), b) if a == b => true,
            // Audio with a large PCM DMA buffer cannot share SRAM with a radio stack.
            (Some(Resource::Audio), Resource::Wifi | Resource::Ble) => false,
            (Some(Resource::Wifi | Resource::Ble), Resource::Audio) => false,
            // Wi-Fi and BLE are not assumed concurrent on this 400 KB C3.
            (Some(Resource::Wifi), Resource::Ble) => false,
            (Some(Resource::Ble), Resource::Wifi) => false,
            _ => false,
        }
    }
}
