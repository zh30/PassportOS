//! Tiny flash KV in the 4 KB page at [`FLASH_KV_OFFSET`]. Not IDF NVS, not cardid.

use esp_println::println;
use esp_storage::FlashStorage;
use passport_core::api::{ApiError, MemoryStore, Store};
use passport_core::board::{FLASH_KV_OFFSET, FLASH_KV_SIZE};
use passport_core::clock::TIME_KEY;
use passport_core::flap::BEST_KEY as FLAP_BEST;
use passport_core::brick::BEST_KEY as BRICK_BEST;
use passport_core::stack::BEST_KEY as STACK_BEST;

const MAGIC: &[u8; 4] = b"POS1";

#[repr(align(4))]
struct Slot([u8; 16]);

pub struct KvStore<'d> {
    ram: MemoryStore,
    flash: FlashStorage<'d>,
}

impl<'d> KvStore<'d> {
    pub fn open(flash: esp_hal::peripherals::FLASH<'d>) -> Self {
        let mut this = Self {
            ram: MemoryStore::new(),
            flash: FlashStorage::new(flash),
        };
        this.load();
        this
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
        println!("[store] saved");
    }
}

impl Store for KvStore<'_> {
    fn get(&self, key: &[u8], dst: &mut [u8]) -> Option<usize> {
        self.ram.get(key, dst)
    }

    fn put(&mut self, key: &[u8], val: &[u8]) -> Result<(), ApiError> {
        let mut old = [0u8; 2];
        let flash_key =
            key == FLAP_BEST || key == TIME_KEY || key == STACK_BEST || key == BRICK_BEST;
        let unchanged = flash_key
            && val.len() >= 2
            && self.ram.get(key, &mut old) == Some(2)
            && old.as_slice() == &val[..2];
        self.ram.put(key, val)?;
        if flash_key && val.len() >= 2 && !unchanged {
            self.persist_slot();
        }
        Ok(())
    }
}
