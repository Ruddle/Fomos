use core::ptr::{read_volatile, write_volatile};

use alloc::{fmt, vec::Vec};
use x86_64::{
    structures::paging::{FrameAllocator, Mapper, Size4KiB},
    PhysAddr, VirtAddr,
};

use crate::{
    create_identity_virt_from_phys,
    pci::{self, Bar, Pci},
    phys_to_virt,
};

pub fn to_bytes<T>(t: &T) -> &[u8] {
    unsafe {
        let len = core::intrinsics::size_of_val(t);
        let ptr: *const u8 = core::intrinsics::transmute(t);
        core::slice::from_raw_parts(ptr, len)
    }
}

const MAX_NUM_QUEUE: usize = 256;
pub struct Virtio {
    pub pci: Pci,

    pub common: VirtioCap<&'static mut VirtioPciCommonCfg>,
    pub device: VirtioCap<&'static mut ()>,
    pub notify: VirtioCap<u32>,
    pub pci_conf: VirtioCap<[u8; 4]>,

    pub step: usize,
    pub last_used_idx: [u16; MAX_NUM_QUEUE],
    pub device_type: DeviceType,
    pub queues_free: Vec<QueueFreeDescs>,
    pub queue_select: u16,
}
pub struct QueueFreeDescs {
    free: Vec<u16>,
}
impl QueueFreeDescs {
    pub fn new(queue_size: u16) -> Self {
        let mut free = Vec::with_capacity(queue_size as usize);
        for i in 0..queue_size {
            free.push(i as u16);
        }
        Self { free }
    }
    pub fn get_free(&mut self) -> Option<u16> {
        self.free.pop()
    }
    pub fn get_free_twice(&mut self) -> Option<(u16, u16)> {
        if self.free.len() >= 2 {
            Some((self.free.pop().unwrap(), self.free.pop().unwrap()))
        } else {
            None
        }
    }
    pub fn set_free(&mut self, desc_id: u16) {
        self.free.push(desc_id);
    }
}

#[derive(Clone, Debug)]
pub enum DeviceType {
    Input,
    Gpu,
}

const DEVICE_ID_INPUT: isize = 18;
const DEVICE_ID_GPU: isize = 16;

fn device_id_to_type(id: isize) -> Option<DeviceType> {
    match id {
        DEVICE_ID_INPUT => Some(DeviceType::Input),
        DEVICE_ID_GPU => Some(DeviceType::Gpu),
        _ => None,
    }
}

impl Virtio {
    pub fn init(
        pci: &Pci,

        mapper: &mut impl Mapper<Size4KiB>,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    ) -> Option<Self> {
        let device_id =
            pci.config_read_u16(pci::PCIConfigRegisters::PCIDeviceID as u8) as isize - 0x1040;

        let device_type = device_id_to_type(device_id);
        if device_type.is_none() {
            return None;
        }
        let device_type = device_type.unwrap();

        let mut bars = [Bar::None; 6];
        for idx in 0..=5 {
            let bar = pci.get_bar(idx);
            if bar != Bar::None {
                // log::info!("bar {}:{:?}", idx, bar);
            }
            bars[idx as usize] = bar;
        }

        let cap_ptr = pci.config_read_u8(pci::PCIConfigRegisters::PCICapabilitiesPointer as u8);

        let mut current_off = cap_ptr;
        const VirtioStatusNone: u8 = 0;
        const VirtioStatusAcknowledge: u8 = 1;
        const VirtioStatusDriver: u8 = 2;
        const VirtioStatusFailed: u8 = 128;
        const VirtioStatusFeatureOk: u8 = 8;
        const VirtioStatusDriverOk: u8 = 4;
        const VirtioStatusNeedsReset: u8 = 64;

        let mut common: Option<VirtioCap<&'static mut VirtioPciCommonCfg>> = None;
        let mut device: Option<VirtioCap<&'static mut ()>> = None;
        let mut notify: Option<VirtioCap<u32>> = None;
        let mut pci_conf: Option<VirtioCap<[u8; 4]>> = None;
        loop {
            let cap = pci.config_read_u8(current_off);

            //VIRTIO
            if cap == 0x9 {
                let cap_len = pci.config_read_u8(current_off + 2);
                let cfg_type = pci.config_read_u8(current_off + 3);
                let bar = pci.config_read_u8(current_off + 4);
                let offset = pci.config_read_u32(current_off + 8);
                let length = pci.config_read_u32(current_off + 12);

                // log::info!(
                //     "virtio {} {} {} {} {}",
                //     cap_len,
                //     cfg_type,
                //     bar,
                //     offset,
                //     length
                // );

                match cfg_type {
                    VirtioPciCapCommonCfg => {
                        if let Bar::Mm(phys) = bars[bar as usize] {
                            let ptr = phys_to_virt(phys + offset as u64);
                            // log::info!("common {:?}", ptr.as_ptr::<VirtioPciCommonCfg>());
                            let cfg: &'static mut VirtioPciCommonCfg =
                                unsafe { &mut *(ptr.as_mut_ptr::<VirtioPciCommonCfg>()) };

                            let x: u8 = unsafe { read_volatile(ptr.as_ptr()) };
                            // log::info!("{}", x);
                            // log::info!("common {:?}", cfg);
                            common = Some(VirtioCap::new(cfg, bars[bar as usize], offset, length));
                        }
                    }
                    VirtioPciCapNotifyCfg => {
                        let notify_off_multiplier = pci.config_read_u32(current_off + 16);
                        notify = Some(VirtioCap {
                            cap: notify_off_multiplier,
                            bar: bars[bar as usize],
                            offset,
                            length,
                        });
                        // log::info!("{:#?}", virtio_caps.notify);
                    }
                    VirtioPciCapIsrCfg => {}
                    VirtioPciCapDeviceCfg => {
                        if let Bar::Mm(phys) = bars[bar as usize] {
                            let ptr: VirtAddr = phys_to_virt(phys + offset as u64);
                            let cfg: &'static mut () = unsafe { &mut *(ptr.as_mut_ptr::<()>()) };
                            device = Some(VirtioCap {
                                cap: cfg,
                                bar: bars[bar as usize],
                                offset,
                                length,
                            });
                        }
                    }
                    VirtioPciCapPciCfg => {
                        let pci_cfg_data = [
                            pci.config_read_u8(current_off + 16),
                            pci.config_read_u8(current_off + 17),
                            pci.config_read_u8(current_off + 18),
                            pci.config_read_u8(current_off + 19),
                        ];

                        pci_conf = Some(VirtioCap {
                            cap: pci_cfg_data,
                            bar: bars[bar as usize],
                            offset,
                            length,
                        });
                    }
                    _ => {}
                }
            }
            // if cap == 0x11 && false {
            //     let line2 = pci.config_read_u32(current_off + 4);
            //     let table_bir = line2 & 0b111;
            //     let table_offset = line2 & 0xFFFFFFf8;
            //     let msg_ctrl = pci.config_read_u16(current_off + 2);
            //     bitfield! {
            //       pub struct MsgCtrl(u16);
            //       impl Debug;
            //       // The fields default to u16
            //       pub table_size, _: 10, 0;
            //       pub reserved, _ : 13, 11;
            //       pub function_mask , _: 14;
            //       pub enable , set_enable: 15;
            //     }

            //     let mut msg_ctrl = MsgCtrl(msg_ctrl);
            //     msg_ctrl.set_enable(true);

            //     pci.config_write_u16(current_off + 2, msg_ctrl.0);
            //     log::info!(
            //         "MSI-X bir:{} tblo:{} msg_ctrl:{:?}",
            //         table_bir,
            //         table_offset,
            //         msg_ctrl
            //     );

            //     let line3 = pci.config_read_u32(current_off + 8);
            //     let pba_bir = line3 & 0b11;
            //     let pba_offset = line3 & 0xFFFFFFf8;

            //     for table_index in 0..=msg_ctrl.table_size() {
            //         if let Bar::Mm(phys) = bars[table_bir as usize] {
            //             let virt_bar = create_virt_from_phys(
            //                 &mut mapper,
            //                 &mut frame_allocator,
            //                 PhysFrame::containing_address(phys),
            //             )
            //             .expect("bar");

            //             // let virt_bar = create_virt_from_phys(
            //             //     &mut mapper,
            //             //     &mut frame_allocator,
            //             //     PhysFrame::containing_address(PhysAddr::new(bars[table_bir as usize])),
            //             // )
            //             // .expect("bar");

            //             let ptr: VirtAddr = virt_bar.start_address()
            //                 + (table_offset as u64)
            //                 + (table_index as u64) * 16;

            //             use core::intrinsics::{volatile_load, volatile_store};

            //             let table = unsafe { volatile_load(ptr.as_ptr() as *const u128) };

            //             bitfield! {
            //               pub struct TableEntry(u128);
            //               impl Debug;
            //               // The fields default to u16
            //               u64, address, set_address : 63, 0;
            //               pub data, set_data : 95, 64;
            //               pub mask, set_mask : 96;
            //               pub reserved , _: 127, 97;

            //             }

            //             let mut table = TableEntry(table);
            //             log::info!("{:?}", table);
            //             table.set_data(vector_base as u128 + table_index as u128);

            //             table.set_address(0xFEE00000 + (0 << 12));
            //             table.set_mask(false);
            //             unsafe { volatile_store(ptr.as_mut_ptr() as *mut u128, table.0) };
            //             // log::info!("{:?}", table);

            //             //READ PENDING
            //             // {
            //             //     let virt_bar = create_virt_from_phys(
            //             //         &mut mapper,
            //             //         &mut frame_allocator,
            //             //         PhysFrame::containing_address(PhysAddr::new(
            //             //             bars[pba_bir as usize],
            //             //         )),
            //             //     )
            //             //     .expect("bar");

            //             //     let ptr: VirtAddr = virt_bar.start_address()
            //             //         + (pba_offset as u64)
            //             //         + (table_index as u64) * 2;

            //             //     let table = unsafe { volatile_load(ptr.as_ptr() as *const u64) };
            //             //     if table != 0 {
            //             //         log::info!("pending {:b}", table);
            //             //     }
            //             // }
            //         }

            //         let command = pci.config_read_u16(4);
            //         log::info!("com {:b}", command);
            //         pci.config_write_u16(4, command | 0b11);

            //         unsafe {
            //             crate::local_apic::LocalApic.get().unwrap().eoi();
            //         };
            //     }
            // }

            current_off = pci.config_read_u8(1 + current_off);

            if current_off == 0 {
                break;
            }
            // break;
        }

        // log::info!("virtio_caps {:?}", virtio_caps);

        if common.is_none() || pci_conf.is_none() || device.is_none() || notify.is_none() {
            return None;
        }

        let mut common = common.unwrap();
        let mut pci_conf = pci_conf.unwrap();
        let mut device = device.unwrap();
        let mut notify = notify.unwrap();

        let cap_common = &mut common.cap;

        unsafe {
            let mut queues = Vec::new();
            write_volatile(&mut cap_common.device_status, 0);

            write_volatile(
                &mut cap_common.device_status,
                read_volatile(&cap_common.device_status) | VirtioStatusAcknowledge,
            );

            write_volatile(
                &mut cap_common.device_status,
                read_volatile(&cap_common.device_status) | VirtioStatusDriver,
            );

            let current = read_volatile(*cap_common);
            log::info!("{:?}", current);
            match device_type {
                DeviceType::Gpu => {
                    write_volatile(&mut cap_common.driver_feature, 0b11);
                }
                _ => {
                    write_volatile(&mut cap_common.driver_feature, 0);
                }
            }

            write_volatile(
                &mut cap_common.device_status,
                read_volatile(&cap_common.device_status) | VirtioStatusFeatureOk,
            );

            if read_volatile(&cap_common.device_status) & VirtioStatusFeatureOk == 0 {
                panic!("Cant enable set of feature")
            }

            for q in 0..cap_common.num_queues {
                write_volatile(&mut cap_common.queue_select, q);

                let q = QueueFreeDescs::new(read_volatile(*cap_common).queue_size);
                queues.push(q);

                // log::info!("q{} len {} ", q, cap.queue_size);

                write_volatile(
                    &mut cap_common.queue_desc,
                    create_identity_virt_from_phys(mapper, frame_allocator)
                        .unwrap()
                        .start_address()
                        .as_u64(),
                );

                {
                    let descs = cap_common.queue_desc as *mut Desc;
                    let qsize = read_volatile(&mut cap_common.queue_size) as isize;
                    log::info!("qsize {} {}", qsize, cap_common.queue_desc);
                    for idesc in 0..qsize {
                        let elem_ptr = descs.offset(idesc);
                        elem_ptr.write_volatile(Desc {
                            addr: create_identity_virt_from_phys(mapper, frame_allocator)
                                .unwrap()
                                .start_address()
                                .as_u64(),
                            // addr: ALLOCATOR.alloc_zeroed(
                            //     Layout::from_size_align_unchecked(4096, 4096),
                            // ) as u64,
                            flags: VIRTQ_DESC_F_WRITE,
                            len: 4096,
                            next: 0xffff,
                        });
                    }
                }

                write_volatile(
                    &mut cap_common.queue_driver,
                    create_identity_virt_from_phys(mapper, frame_allocator)
                        .unwrap()
                        .start_address()
                        .as_u64(),
                );

                //DISABLE DEVICE TO DRIVER NOTIFICATION (Interrupt)
                (cap_common.queue_driver as *mut u16).write_volatile(1);

                write_volatile(
                    &mut cap_common.queue_device,
                    create_identity_virt_from_phys(mapper, frame_allocator)
                        .unwrap()
                        .start_address()
                        .as_u64(),
                );

                // (cap.queue_device as *mut u16).write_volatile(1);

                write_volatile(&mut cap_common.queue_enable, 1);
            }

            let cap_device = &mut device.cap;

            match device_type {
                DeviceType::Input => {
                    let conf_ptr: *mut VirtioInputConfig =
                        core::intrinsics::transmute((*cap_device) as *mut ());
                    let rconf = read_volatile(conf_ptr);
                    let conf: &mut VirtioInputConfig = conf_ptr.as_mut().unwrap();
                    write_volatile(&mut conf.select, 1);
                    let u = read_volatile((&conf.u));
                    log::info!(
                        "name: {:?}",
                        alloc::str::from_utf8_unchecked(
                            &u.bitmap[0..read_volatile(&conf.size) as usize]
                        )
                    );
                    write_volatile(&mut conf.select, 0);
                }
                DeviceType::Gpu => {
                    #[repr(C)]
                    #[derive(Clone, Debug)]
                    struct VirtioGpuConfig {
                        events_read: u32,
                        events_clear: u32,
                        num_scanouts: u32,
                        num_capsets: u32,
                    }
                    let conf_ptr: *mut VirtioGpuConfig =
                        core::intrinsics::transmute((*cap_device) as *mut ());
                    let mut rconf = conf_ptr.read_volatile();
                    log::info!("{:?}", rconf);
                    // rconf.events_clear = 1;
                    // conf_ptr.write_volatile(rconf);
                }
                _ => {}
            }

            write_volatile(
                &mut cap_common.device_status,
                read_volatile(&cap_common.device_status) | VirtioStatusDriverOk,
            );

            let mut this = Self {
                pci: pci.clone(),
                last_used_idx: [u16::MAX; MAX_NUM_QUEUE],
                step: 0,

                device_type,
                queues_free: queues,

                common,
                device,
                notify,
                pci_conf,
                queue_select: 0,
            };
            this.queue_select(0);
            Some(this)
        }
    }

    pub fn get_free_desc_id(&mut self) -> Option<u16> {
        self.queues_free[self.queue_select as usize].get_free()
    }
    pub fn get_free_twice_desc_id(&mut self) -> Option<(u16, u16)> {
        self.queues_free[self.queue_select as usize].get_free_twice()
    }
    pub fn set_free_desc_id(&mut self, desc_id: u16) {
        self.queues_free[self.queue_select as usize].set_free(desc_id);
    }

    pub fn queue_select(&mut self, q: u16) {
        unsafe {
            self.queue_select = q;
            write_volatile(&mut self.common.cap.queue_select, q);
        }
    }
    pub fn set_available(&mut self, desc_id: u16) {
        unsafe {
            let queue = read_volatile(self.common.cap);
            let driver_idx = (self.common.cap.queue_driver as *mut u8).offset(2) as *mut u16;
            let driver_ring_start = (self.common.cap.queue_driver as *mut u8).offset(4) as *mut u16;
            let idx = driver_idx.read_volatile();
            let elem_ptr = driver_ring_start.offset(idx as isize % queue.queue_size as isize);
            elem_ptr.write_volatile(desc_id);
            driver_idx.write_volatile(idx.wrapping_add(1));
        }
    }

    pub fn set_writable(&mut self, desc_id: u16) {
        unsafe {
            let descs = self.common.cap.queue_desc as *mut Desc;
            let mut desc = descs.offset(desc_id as isize).read_volatile();
            desc.flags = VIRTQ_DESC_F_WRITE;
            desc.len = 4096;
            descs.offset(desc_id as isize).write_volatile(desc);
        }
    }

    pub fn set_writable_available(&mut self, desc_id: u16) {
        self.set_writable(desc_id);
        self.set_available(desc_id);
    }

    pub fn add_request<T>(&mut self, desc_id: u16, desc_next_id: u16, data: T) {
        unsafe {
            let descs = self.common.cap.queue_desc as *mut Desc;
            let mut desc = descs.offset(desc_id as isize).read_volatile();
            desc.len = core::intrinsics::size_of_val(&data) as u32;
            // desc.len = data.len() as u32;
            let data_ptr = desc.addr as *mut T;
            data_ptr.write_volatile(data);

            desc.flags = VIRTQ_DESC_F_NEXT;
            desc.next = desc_next_id;
            descs.offset(desc_id as isize).write_volatile(desc);
            self.set_writable(desc_next_id);
            self.set_available(desc_id);
        };
    }

    pub fn kick(&mut self, queue_select: u16) {
        unsafe {
            let queue = read_volatile(self.common.cap);
            let VirtioCap {
                cap: cap_notify,
                bar,
                offset: offset_notify,
                length,
            } = &mut self.notify;

            if let Bar::Mm(addr) = bar {
                let queue_notify_address = phys_to_virt(PhysAddr::new(
                    addr.as_u64()
                        + (*offset_notify as u64)
                        + (*cap_notify as u64) * (queue.queue_notify_off as u64),
                ));

                // log::info!("kick at {:?}", queue_notify_address);
                (queue_notify_address.as_u64() as *mut u16).write_volatile(queue_select);
            }
        }
    }

    pub unsafe fn next_used(&mut self) -> Option<UsedElem> {
        let queue = read_volatile(self.common.cap);

        let device_idx = (self.common.cap.queue_device as *mut u8).offset(2) as *mut u16;
        let idx_next = device_idx.read_volatile();
        let device_ring_start =
            (self.common.cap.queue_device as *mut u8).offset(4) as *mut UsedElem;

        let last_used_idx = &mut self.last_used_idx[self.queue_select as usize];
        if last_used_idx.wrapping_add(1) != idx_next {
            *last_used_idx = last_used_idx.wrapping_add(1);
            let inq_idx = (*last_used_idx as isize) % queue.queue_size as isize;
            let elem_ptr = device_ring_start.offset(inq_idx);
            let elem = read_volatile(elem_ptr);
            Some(elem)
        } else {
            None
        }
    }

    pub fn read_desc(&mut self, desc_id: u16) -> Desc {
        unsafe {
            let descs = self.common.cap.queue_desc as *mut Desc;
            descs.offset(desc_id as isize).read_volatile()
        }
    }
}

#[repr(C)]
#[derive(Debug, PartialEq)]
pub struct VirtioPciCommonCfg {
    // About the whole device.
    pub device_feature_select: u32, // read-write
    pub device_feature: u32,        // read-only for driver
    pub driver_feature_select: u32, // read-write
    pub driver_feature: u32,        // read-write
    pub msix_config: u16,           // read-write
    pub num_queues: u16,            // read-only for driver
    pub device_status: u8,          // read-write
    pub config_generation: u8,      // read-only for driver

    // About a specific virtqueue.
    pub queue_select: u16,      // read-write
    pub queue_size: u16,        // read-write
    pub queue_msix_vector: u16, // read-write
    pub queue_enable: u16,      // read-write
    pub queue_notify_off: u16,  // read-only for driver
    pub queue_desc: u64,        // read-write
    pub queue_driver: u64,      // read-write
    pub queue_device: u64,      // read-write
}
#[derive(Debug, PartialEq)]
pub struct VirtioCap<T> {
    pub cap: T,
    pub bar: Bar,
    pub offset: u32,
    pub length: u32,
}

impl<T> VirtioCap<T> {
    pub fn new(t: T, bar: Bar, offset: u32, length: u32) -> Self {
        Self {
            cap: t,
            bar,
            offset,
            length,
        }
    }
}

const VirtioPciCapCommonCfg: u8 = 1;
const VirtioPciCapNotifyCfg: u8 = 2;
const VirtioPciCapIsrCfg: u8 = 3;
const VirtioPciCapDeviceCfg: u8 = 4;
const VirtioPciCapPciCfg: u8 = 5;

// Device cfg
#[repr(C)]
#[derive(Debug)]
struct VirtioInputConfig {
    select: u8,
    subsel: u8,
    size: u8,
    reserved: [u8; 5],
    u: VirtioInputUnion,
}
#[repr(C)]
#[derive(Clone, Copy)]
union VirtioInputUnion {
    string: [char; 128],
    bitmap: [u8; 128],
    abs: VirtioInputAbsInfo,
    ids: VirtioInputDevIds,
}

impl fmt::Debug for VirtioInputUnion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        unsafe {
            f.debug_struct("VirtioInputUnion")
                .field("abs", &self.abs)
                .field("ids", &self.ids)
                .finish()
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct VirtioInputAbsInfo {
    min: u32,
    max: u32,
    fuzz: u32,
    flat: u32,
    resolution: u32,
}
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct VirtioInputDevIds {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;
const VIRTQ_DESC_F_INDIRECT: u16 = 4;
//Queue handle
#[repr(C, align(16))]
#[derive(Clone, Debug, PartialEq)]
pub struct Desc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

#[repr(C)]
#[derive(Debug)]
pub struct UsedElem {
    pub id: u32,
    pub len: u32,
}
