use core::fmt::Debug;

use alloc::vec::Vec;
use x86_64::{
    instructions::port::{Port, PortGeneric, ReadWriteAccess},
    PhysAddr,
};
pub enum PCIConfigRegisters {
    PCIDeviceID = 0x2,
    PCIVendorID = 0x0,
    PCIStatus = 0x6,
    PCICommand = 0x4,
    PCIClassCode = 0xB,
    PCISubclass = 0xA,
    PCIProgIF = 0x9,
    PCIRevisionID = 0x8,
    PCIBIST = 0xF,
    PCIHeaderType = 0xE,
    PCILatencyTimer = 0xD,
    PCICacheLineSize = 0xC,
    PCIBAR0 = 0x10,
    PCIBAR1 = 0x14,
    PCIBAR2 = 0x18,
    PCIBAR3 = 0x1C,
    PCIBAR4 = 0x20,
    PCIBAR5 = 0x24,
    PCICardbusCISPointer = 0x28,
    PCISubsystemID = 0x2E,
    PCISubsystemVendorID = 0x2C,
    PCIExpansionROMBaseAddress = 0x30,
    PCICapabilitiesPointer = 0x34,
    PCIMaxLatency = 0x3F,
    PCIMinGrant = 0x3E,
    PCIInterruptPIN = 0x3D,
    PCIInterruptLine = 0x3C,
}

const port_config_address: PortGeneric<u32, ReadWriteAccess> = Port::new(0xCF8);
const port_config_data: PortGeneric<u32, ReadWriteAccess> = Port::new(0xCFC);
const port_config_data_u8: PortGeneric<u8, ReadWriteAccess> = Port::new(0xCFC);
pub fn config_address(bus: u8, slot: u8, func: u8, off: u8) {
    let address: u32 = (((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((func as u32) << 8)
        | ((off as u32) & 0xfc)
        | 0x80000000);

    unsafe {
        port_config_address.write(address);
    }
}
pub fn config_read_u32(bus: u8, slot: u8, func: u8, off: u8) -> u32 {
    config_address(bus, slot, func, off);
    let read: u32 = unsafe { port_config_data.read() };
    read
}

pub fn config_read_u16(bus: u8, slot: u8, func: u8, off: u8) -> u16 {
    config_address(bus, slot, func, off);

    let read: u32 = unsafe { port_config_data.read() };
    let read = (read >> ((off & 2) * 8)) & 0xffff;
    read as u16
}

pub fn config_read_u8(bus: u8, slot: u8, func: u8, off: u8) -> u8 {
    config_address(bus, slot, func, off);

    // unsafe { port_config_data_u8.read() }
    let read: u32 = unsafe { port_config_data.read() };
    let read = (read >> ((off & 3) * 8)) & 0xff;
    read as u8
}

pub fn config_write_u32(bus: u8, slot: u8, func: u8, off: u8, data: u32) {
    config_address(bus, slot, func, off);
    unsafe {
        port_config_data.write(data);
    };
}

pub fn config_write_u16(bus: u8, slot: u8, func: u8, off: u8, data: u16) {
    config_address(bus, slot, func, off);

    let read: u32 = unsafe { port_config_data.read() };
    let val = (read & (!(0xFFFF << ((off & 2) * 8)))) | ((data as u32) << ((off & 2) * 8));
    unsafe {
        port_config_data.write(val);
    };
}

pub fn config_write_u8(bus: u8, slot: u8, func: u8, off: u8, data: u8) {
    config_address(bus, slot, func, off);

    let read: u32 = unsafe { port_config_data.read() };
    let val = (read & (!(0xFF << ((off & 3) * 8)))) | ((data as u32) << ((off & 3) * 8));
    unsafe {
        port_config_data.write(val);
    };
}
#[derive(Clone)]
pub struct Pci {
    pub bus: u8,
    pub slot: u8,
    pub func: u8,
}

impl Debug for Pci {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "pci:{:02x}:{:02x}:{:02x}",
            self.bus, self.slot, self.func
        )
    }
}

#[derive(Clone, Debug, Copy, PartialEq)]
pub enum Bar {
    None,
    Io(u32),
    Mm(PhysAddr),
}

impl Pci {
    pub fn config_read_u8(&self, off: u8) -> u8 {
        config_read_u8(self.bus, self.slot, self.func, off as u8)
    }
    pub fn config_write_u8(&self, off: u8, val: u8) {
        config_write_u8(self.bus, self.slot, self.func, off as u8, val)
    }
    pub fn config_read_u16(&self, off: u8) -> u16 {
        config_read_u16(self.bus, self.slot, self.func, off as u8)
    }
    pub fn config_write_u16(&self, off: u8, val: u16) {
        config_write_u16(self.bus, self.slot, self.func, off as u8, val)
    }
    pub fn config_read_u32(&self, off: u8) -> u32 {
        config_read_u32(self.bus, self.slot, self.func, off as u8)
    }

    pub fn get_bar(&self, idx: u8) -> Bar {
        fn idx_to_enum(idx: u8) -> PCIConfigRegisters {
            match idx {
                0 => PCIConfigRegisters::PCIBAR0,
                1 => PCIConfigRegisters::PCIBAR1,
                2 => PCIConfigRegisters::PCIBAR2,
                3 => PCIConfigRegisters::PCIBAR3,
                4 => PCIConfigRegisters::PCIBAR4,
                _ => PCIConfigRegisters::PCIBAR5,
            }
        }
        let mut bar = self.config_read_u32(idx_to_enum(idx) as u8) as u64;
        if bar == 0 {
            return Bar::None;
        }
        let io = bar & 0x1 != 0;

        if io {
            return Bar::Io((bar & 0xFFFFFFFFFFFFFFFC) as u32);
        }

        let bit64 = bar & 0x4 != 0;
        let not_last = idx < 5;
        if bit64 && not_last {
            bar |= (config_read_u32(
                self.bus,
                self.slot,
                self.func,
                PCIConfigRegisters::PCIBAR0 as u8 + ((idx as u8 + 1) * 4),
            ) as u64)
                << 32;
        }
        let masked = bar & 0xFFFFFFFFFFFFFFF0;
        if masked == 0 {
            return Bar::None;
        }
        Bar::Mm(PhysAddr::new(masked))
    }
    pub fn get_irq(&self) -> u8 {
        self.config_read_u8(PCIConfigRegisters::PCIInterruptLine as u8)
    }
    pub fn get_ipin(&self) -> u8 {
        self.config_read_u8(PCIConfigRegisters::PCIInterruptPIN as u8)
    }
}

pub struct Pcis {
    pub devs: Vec<Pci>,
}

impl Pcis {
    pub fn new() -> Self {
        let mut devs = Vec::new();
        for bus in 0..=255 {
            for slot in 0..32 {
                for func in 0..8 {
                    let vendor =
                        config_read_u16(bus, slot, func, PCIConfigRegisters::PCIVendorID as u8);
                    if vendor != 0xFFFF {
                        let device_id =
                            config_read_u16(bus, slot, func, PCIConfigRegisters::PCIDeviceID as u8);
                        let header_type = config_read_u8(
                            bus,
                            slot,
                            func,
                            PCIConfigRegisters::PCIHeaderType as u8,
                        );
                        // log::trace!(
                        //     "{:x}:{:x}:{:x} vendor: {:x} id:{:x} ht: {:x}",
                        //     bus,
                        //     slot,
                        //     func,
                        //     vendor,
                        //     device_id,
                        //     header_type
                        // );

                        devs.push(Pci { bus, slot, func });
                        if func == 0 && (header_type & 0x80) == 0 {
                            break;
                        }
                    }
                }
            }
        }
        Self { devs }
    }
}
