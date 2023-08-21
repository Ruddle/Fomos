use core::intrinsics::{volatile_load, volatile_store};

use conquer_once::spin::OnceCell;
use x86_64::{structures::DescriptorTablePointer, PhysAddr, VirtAddr};

use crate::phys_to_virt;
pub static IO_APIC_0: OnceCell<IoApic> = OnceCell::uninit();

pub struct IoApic {
    virt_address: VirtAddr,
    global_system_int: u32,
    id: u8,
}

pub const IOAPICID: u32 = 0;
pub const IOAPICVER: u32 = 1;
impl IoApic {
    pub fn init(info: &acpi::platform::interrupt::IoApic) -> &Self {
        let this = IO_APIC_0.get_or_init(move || Self {
            id: info.id,
            virt_address: phys_to_virt(PhysAddr::new(info.address as u64)),
            global_system_int: info.global_system_interrupt_base,
        });
        this
    }
    pub unsafe fn set_sel(&self, reg: u32) {
        volatile_store(self.virt_address.as_u64() as *mut u32, reg);
    }
    pub fn read(&self, reg: u32) -> u32 {
        unsafe {
            self.set_sel(reg);
            let sec: VirtAddr = self.virt_address + 0x10_u64;
            volatile_load(sec.as_u64() as *const u32)
        }
    }
    pub fn write(&self, reg: u32, value: u32) {
        unsafe {
            self.set_sel(reg);
            let sec: VirtAddr = self.virt_address + 0x10_u64;
            volatile_store(sec.as_u64() as *mut u32, value);
        }
    }

    pub fn read_redtlb(&self, index: u32) -> u64 {
        let low = self.read(0x10 + 2 * index) as u64;
        let high = self.read(0x10 + 2 * index + 1) as u64;
        (high << 32) + low
    }
    pub fn write_redtlb(&self, index: u32, redtlb: u64) {
        let low = self.write(0x10 + 2 * index, (redtlb & 0xffff) as u32);
        let high = self.write(0x10 + 2 * index + 1, (redtlb >> 32) as u32);
    }
}
#[derive(Clone, Debug)]
pub struct RedTbl {
    pub vector: u8,
    pub delivery_mode: u8,
    pub destination_mode: bool,
    pub delivery_status: bool,
    pub pin_polarity: bool,
    pub remote_irr: bool,
    pub trigger_mode: bool,
    pub mask: bool,
    pub destination: u8,
}

impl RedTbl {
    pub fn new(n: u64) -> Self {
        let mut c = n;
        let vector = (c & 0xff) as u8;
        c >>= 8;
        let delivery_mode = (c & 0b11) as u8;
        c >>= 2;
        let destination_mode = c & 0b1 != 0;
        c >>= 1;
        let delivery_status = c & 0b1 != 0;
        c >>= 1;
        let pin_polarity = c & 0b1 != 0;
        c >>= 1;
        let remote_irr = c & 0b1 != 0;
        c >>= 1;
        let trigger_mode = c & 0b1 != 0;
        c >>= 1;
        let mask = c & 0b1 != 0;
        c >>= 1;
        let destination = (n >> 56) as u8;
        Self {
            vector,
            delivery_mode,
            destination_mode,
            delivery_status,
            pin_polarity,
            remote_irr,
            trigger_mode,
            mask,
            destination,
        }
    }
    pub fn store(&self) -> u64 {
        let &Self {
            vector,
            delivery_mode,
            destination_mode,
            delivery_status,
            pin_polarity,
            remote_irr,
            trigger_mode,
            mask,
            destination,
        } = self;

        let mut r = 0_u64;
        r += (destination as u64) << 56;
        r += vector as u64;
        r += (delivery_mode as u64) << 8;
        r += if destination_mode { 1 } else { 0 } << 10;
        r += if delivery_status { 1 } else { 0 } << 11;
        r += if pin_polarity { 1 } else { 0 } << 12;
        r += if remote_irr { 1 } else { 0 } << 13;
        r += if trigger_mode { 1 } else { 0 } << 14;
        r += if mask { 1 } else { 0 } << 15;
        r
    }
}
