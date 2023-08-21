use conquer_once::spin::OnceCell;
use core::intrinsics::{volatile_load, volatile_store};
use raw_cpuid::{CpuId, CpuIdResult};
use x86_64::{
    registers::model_specific::Msr,
    structures::paging::{FrameAllocator, Size4KiB},
    PhysAddr, VirtAddr,
};

use crate::phys_to_virt;

pub fn cpuid() -> Option<CpuId> {
    //TODO: ensure that CPUID exists! https://wiki.osdev.org/CPUID#Checking_CPUID_availability
    Some(CpuId::with_cpuid_fn(|a, c| {
        let result = unsafe { core::arch::x86_64::__cpuid_count(a, c) };
        CpuIdResult {
            eax: result.eax,
            ebx: result.ebx,
            ecx: result.ecx,
            edx: result.edx,
        }
    }))
}

pub static LOCAL_APIC: OnceCell<LocalApic> = OnceCell::uninit();

pub struct LocalApic {
    pub virt_address: VirtAddr,
}

impl LocalApic {
    pub unsafe fn init(local_apic_address: PhysAddr) -> &'static Self {
        disable_pic();

        let virtaddr: VirtAddr = phys_to_virt(local_apic_address);
        let this = LOCAL_APIC.get_or_init(|| Self {
            virt_address: virtaddr,
        });
        log::info!("LocalApic {:?}", virtaddr);

        let mut msr = Msr::new(0x1B);
        let r = msr.read();
        msr.write(r | (1 << 11));

        this.write(0xF0, this.read(0xF0) | 0x1FF);
        this
    }

    unsafe fn read(&self, reg: u32) -> u32 {
        volatile_load((self.virt_address.as_u64() + reg as u64) as *const u32)
    }

    unsafe fn write(&self, reg: u32, value: u32) {
        volatile_store((self.virt_address.as_u64() + reg as u64) as *mut u32, value);
    }

    pub fn id(&self) -> u32 {
        unsafe { self.read(0x20) }
    }

    pub fn version(&self) -> u32 {
        unsafe { self.read(0x30) }
    }

    pub fn icr(&self) -> u64 {
        unsafe { (self.read(0x310) as u64) << 32 | self.read(0x300) as u64 }
    }

    pub fn set_icr(&self, value: u64) {
        unsafe {
            const PENDING: u32 = 1 << 12;
            while self.read(0x300) & PENDING == PENDING {
                core::hint::spin_loop();
            }
            self.write(0x310, (value >> 32) as u32);
            self.write(0x300, value as u32);
            while self.read(0x300) & PENDING == PENDING {
                core::hint::spin_loop();
            }
        }
    }

    pub fn ipi(&self, apic_id: usize) {
        let mut icr = 0x4040;

        icr |= (apic_id as u64) << 56;

        self.set_icr(icr);
    }
    // Not used just yet, but allows triggering an NMI to another processor.
    pub fn ipi_nmi(&self, apic_id: u32) {
        let shift = { 56 };
        self.set_icr((u64::from(apic_id) << shift) | (1 << 14) | (0b100 << 8));
    }

    pub unsafe fn eoi(&self) {
        self.write(0xB0, 0);
    }
    /// Reads the Error Status Register.
    pub unsafe fn esr(&self) -> u32 {
        self.write(0x280, 0);
        self.read(0x280)
    }
    pub unsafe fn lvt_timer(&self) -> u32 {
        self.read(0x320)
    }
    pub unsafe fn set_lvt_timer(&self, value: u32) {
        self.write(0x320, value);
    }
    pub unsafe fn init_count(&self) -> u32 {
        self.read(0x380)
    }
    pub unsafe fn set_init_count(&self, initial_count: u32) {
        self.write(0x380, initial_count);
    }
    pub unsafe fn cur_count(&self) -> u32 {
        self.read(0x390)
    }
    pub unsafe fn div_conf(&self) -> u32 {
        self.read(0x3E0)
    }
    pub unsafe fn set_div_conf(&self, div_conf: u32) {
        self.write(0x3E0, div_conf);
    }
    pub unsafe fn lvt_error(&self) -> u32 {
        self.read(0x370)
    }
    pub unsafe fn set_lvt_error(&self, lvt_error: u32) {
        self.write(0x370, lvt_error);
    }
    unsafe fn setup_error_int(&self) {
        let vector = 49u32;
        self.set_lvt_error(vector);
    }
}
pub unsafe fn disable_pic() {
    use x86_64::instructions::port::Port;
    let mut wait_port: Port<u8> = Port::new(0x80);
    let mut wait = || {
        wait_port.write(0);
    };
    let mut p0c: Port<u8> = Port::new(0x20);
    let mut p0d: Port<u8> = Port::new(0x21);
    let mut p1c: Port<u8> = Port::new(0xA0);
    let mut p1d: Port<u8> = Port::new(0xA1);
    p0c.write(0x11);
    wait();
    p1c.write(0x11);
    wait();

    //OFFSET
    p0d.write(0xf0);
    wait();
    p1d.write(0xf8);
    wait();
    //CHAINING
    p0d.write(0x4);
    wait();
    p1d.write(0x2);
    wait();
    //ICW4_8086 MODE
    p0d.write(0x1);
    wait();
    p1d.write(0x1);
    wait();
    //CLOSE
    p0d.write(0xff);
    wait();
    p1d.write(0xff);
    wait();
}
