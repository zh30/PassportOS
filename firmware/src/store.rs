//! Tiny flash KV in the 4 KB page at [`FLASH_KV_OFFSET`]. Not IDF NVS, not cardid.

use esp_println::println;
use esp_storage::FlashStorage;
use passport_core::api::{ApiError, MemoryStore, Store};
use passport_core::board::{FLASH_APP_OFFSET, FLASH_KV_OFFSET, FLASH_KV_SIZE};
use passport_core::boo::BEST_KEY as BOO_BEST;
use passport_core::brick::BEST_KEY as BRICK_BEST;
use passport_core::clock::TIME_KEY;
use passport_core::factory_image_len;
use passport_core::flap::BEST_KEY as FLAP_BEST;
use passport_core::stack::BEST_KEY as STACK_BEST;
use passport_core::wifi::{WIFI_OPEN_KEY, WIFI_PASS_KEY, WIFI_SSID_KEY};

const MAGIC: &[u8; 4] = b"POS1";
const MAGIC2: &[u8; 4] = b"POS2";
/// Saved Wi-Fi entry record at offset 32 in the KV page.
const MAGICW: &[u8; 4] = b"POSW";
const WIFI_REC_OFF: u32 = 32;
/// magic(4) ssid_len(1) pass_len(1) flags(1) crc(1) ssid(32) pass(64) pad → 108
const WIFI_REC_LEN: usize = 108;

#[repr(align(4))]
struct Slot([u8; 16]);

#[repr(align(4))]
struct WifiRec([u8; WIFI_REC_LEN]);

pub struct KvStore<'d> {
    ram: MemoryStore,
    flash: FlashStorage<'d>,
    /// Wi-Fi key writes are batched; `flush` erases+writes the page once.
    wifi_dirty: bool,
}

impl<'d> KvStore<'d> {
    pub fn open(flash: esp_hal::peripherals::FLASH<'d>) -> Self {
        let mut this = Self {
            ram: MemoryStore::new(),
            flash: FlashStorage::new(flash),
            wifi_dirty: false,
        };
        this.load();
        this
    }

    /// Factory image length from the 0xE9 header at [`FLASH_APP_OFFSET`].
    pub fn factory_image_bytes(&mut self) -> Option<u32> {
        factory_image_len(|off, buf| {
            self.flash
                .read_nor(FLASH_APP_OFFSET.saturating_add(off), buf)
                .is_ok()
        })
    }

    fn load(&mut self) {
        let mut slot = Slot([0; 16]);
        if self.flash.read_nor(FLASH_KV_OFFSET, &mut slot.0).is_err() {
            println!("[store] read miss");
            return;
        }
        if &slot.0[..4] != MAGIC {
            println!("[store] empty");
            return;
        }
        let sum = slot.0[0] ^ slot.0[1] ^ slot.0[2] ^ slot.0[3] ^ slot.0[4] ^ slot.0[5];
        if slot.0[6] != sum {
            println!("[store] bad crc");
            return;
        }
        let best = u16::from_le_bytes([slot.0[4], slot.0[5]]);
        let _ = self.ram.put(FLAP_BEST, &best.to_le_bytes());
        println!("[store] flap best={best}");
        if slot.0[13] == slot.0[11] ^ slot.0[12] {
            let stk = u16::from_le_bytes([slot.0[11], slot.0[12]]);
            let _ = self.ram.put(STACK_BEST, &stk.to_le_bytes());
            if stk > 0 {
                println!("[store] stack best={stk}");
            }
        }
        let brk = u16::from_le_bytes([slot.0[14], slot.0[15]]);
        if brk != 0xFFFF {
            let _ = self.ram.put(BRICK_BEST, &brk.to_le_bytes());
            if brk > 0 {
                println!("[store] brick best={brk}");
            }
        }
        if slot.0[7] & 1 != 0 {
            let xor = slot.0[7] ^ slot.0[8] ^ slot.0[9];
            if slot.0[10] == xor {
                let mins = u16::from_le_bytes([slot.0[8], slot.0[9]]);
                if mins < 24 * 60 {
                    let _ = self.ram.put(TIME_KEY, &mins.to_le_bytes());
                    println!("[store] time {mins}min");
                }
            }
        }
        let mut extra = Slot([0; 16]);
        if self
            .flash
            .read_nor(FLASH_KV_OFFSET + 16, &mut extra.0)
            .is_ok()
            && &extra.0[..4] == MAGIC2
        {
            let sum = extra.0[0] ^ extra.0[1] ^ extra.0[2] ^ extra.0[3] ^ extra.0[4] ^ extra.0[5];
            if extra.0[6] == sum {
                let boo = u16::from_le_bytes([extra.0[4], extra.0[5]]);
                let _ = self.ram.put(BOO_BEST, &boo.to_le_bytes());
                if boo > 0 {
                    println!("[store] boo best={boo}");
                }
            }
        }
        let mut wrec = WifiRec([0; WIFI_REC_LEN]);
        if self
            .flash
            .read_nor(FLASH_KV_OFFSET + WIFI_REC_OFF, &mut wrec.0)
            .is_ok()
            && &wrec.0[..4] == MAGICW
        {
            let slen = wrec.0[4] as usize;
            let plen = wrec.0[5] as usize;
            let flags = wrec.0[6];
            let crc = wrec.0[7];
            let mut sum = 0u8;
            for b in &wrec.0[4..7] {
                sum ^= *b;
            }
            for i in 0..slen.min(32) {
                sum ^= wrec.0[8 + i];
            }
            for i in 0..plen.min(64) {
                sum ^= wrec.0[40 + i];
            }
            if crc == sum && slen > 0 && slen <= 32 && plen <= 64 {
                let _ = self.ram.put(WIFI_SSID_KEY, &wrec.0[8..8 + slen]);
                let _ = self.ram.put(WIFI_PASS_KEY, &wrec.0[40..40 + plen]);
                let _ = self.ram.put(WIFI_OPEN_KEY, &[flags & 1]);
                println!(
                    "[store] wifi ssid={}",
                    core::str::from_utf8(&wrec.0[8..8 + slen]).unwrap_or("?")
                );
            } else {
                println!("[store] wifi rec bad crc");
            }
        }
    }

    fn build_wifi_rec(&self) -> WifiRec {
        let mut rec = WifiRec([0; WIFI_REC_LEN]);
        rec.0[..4].copy_from_slice(MAGICW);
        let mut ssid = [0u8; 32];
        let slen = self
            .ram
            .get(WIFI_SSID_KEY, &mut ssid)
            .unwrap_or(0)
            .min(32);
        let mut pass = [0u8; 64];
        let plen = self
            .ram
            .get(WIFI_PASS_KEY, &mut pass)
            .unwrap_or(0)
            .min(64);
        let mut open = [0u8; 1];
        let open = self.ram.get(WIFI_OPEN_KEY, &mut open) == Some(1) && open[0] == 1;
        rec.0[4] = slen as u8;
        rec.0[5] = plen as u8;
        rec.0[6] = open as u8;
        rec.0[8..8 + slen].copy_from_slice(&ssid[..slen]);
        rec.0[40..40 + plen].copy_from_slice(&pass[..plen]);
        let mut sum = 0u8;
        for b in &rec.0[4..7] {
            sum ^= *b;
        }
        for i in 0..slen {
            sum ^= rec.0[8 + i];
        }
        for i in 0..plen {
            sum ^= rec.0[40 + i];
        }
        rec.0[7] = sum;
        rec
    }

    fn persist_slot(&mut self) {
        let mut slot = Slot([0; 16]);
        slot.0[..4].copy_from_slice(MAGIC);
        let mut best = [0u8; 2];
        if self.ram.get(FLAP_BEST, &mut best) == Some(2) {
            slot.0[4..6].copy_from_slice(&best);
        }
        slot.0[6] = slot.0[0] ^ slot.0[1] ^ slot.0[2] ^ slot.0[3] ^ slot.0[4] ^ slot.0[5];
        let mut tod = [0u8; 2];
        if self.ram.get(TIME_KEY, &mut tod) == Some(2) {
            slot.0[7] = 1;
            slot.0[8..10].copy_from_slice(&tod);
            slot.0[10] = slot.0[7] ^ slot.0[8] ^ slot.0[9];
        }
        let mut stk = [0u8; 2];
        if self.ram.get(STACK_BEST, &mut stk) == Some(2) {
            slot.0[11..13].copy_from_slice(&stk);
        }
        slot.0[13] = slot.0[11] ^ slot.0[12];
        let mut brk = [0u8; 2];
        if self.ram.get(BRICK_BEST, &mut brk) == Some(2) {
            slot.0[14..16].copy_from_slice(&brk);
        }
        let mut extra = Slot([0; 16]);
        extra.0[..4].copy_from_slice(MAGIC2);
        let mut boo = [0u8; 2];
        if self.ram.get(BOO_BEST, &mut boo) == Some(2) {
            extra.0[4..6].copy_from_slice(&boo);
        }
        extra.0[6] = extra.0[0] ^ extra.0[1] ^ extra.0[2] ^ extra.0[3] ^ extra.0[4] ^ extra.0[5];
        if self
            .flash
            .erase(FLASH_KV_OFFSET, FLASH_KV_OFFSET + FLASH_KV_SIZE)
            .is_err()
        {
            println!("[store] erase fail");
            return;
        }
        if self.flash.write_nor(FLASH_KV_OFFSET, &slot.0).is_err() {
            println!("[store] write fail");
            return;
        }
        if self
            .flash
            .write_nor(FLASH_KV_OFFSET + 16, &extra.0)
            .is_err()
        {
            println!("[store] write2 fail");
            return;
        }
        let wrec = self.build_wifi_rec();
        if self
            .flash
            .write_nor(FLASH_KV_OFFSET + WIFI_REC_OFF, &wrec.0)
            .is_err()
        {
            println!("[store] write wifi fail");
            return;
        }
        println!("[store] saved");
    }

    /// Persist deferred Wi-Fi key writes in one page erase+write cycle.
    pub fn flush(&mut self) {
        if self.wifi_dirty {
            self.wifi_dirty = false;
            self.persist_slot();
        }
    }
}

impl Store for KvStore<'_> {
    fn get(&self, key: &[u8], dst: &mut [u8]) -> Option<usize> {
        self.ram.get(key, dst)
    }

    fn put(&mut self, key: &[u8], val: &[u8]) -> Result<(), ApiError> {
        let wifi_key = key == WIFI_SSID_KEY || key == WIFI_PASS_KEY || key == WIFI_OPEN_KEY;
        let flash_key = wifi_key
            || key == FLAP_BEST
            || key == TIME_KEY
            || key == STACK_BEST
            || key == BRICK_BEST
            || key == BOO_BEST;
        let mut old = [0u8; 64];
        let unchanged = flash_key
            && self
                .ram
                .get(key, &mut old)
                .is_some_and(|n| old[..n] == *val);
        self.ram.put(key, val)?;
        if wifi_key && !unchanged {
            self.wifi_dirty = true;
        } else if flash_key && !unchanged {
            self.persist_slot();
        }
        Ok(())
    }
}
