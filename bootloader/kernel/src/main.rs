#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(core_intrinsics)]

use acpi::{AcpiHandler, HpetInfo, InterruptModel, PhysicalMapping};
use alloc::{boxed::Box, fmt, format, slice, string::String, sync::Arc, vec::Vec};
extern crate alloc;
extern crate edid_rs;
use arrayvec::ArrayVec;
use bitfield::bitfield;
use bootloader_api::{entry_point, info::FrameBufferInfo, BootInfo};
use bootloader_boot_config::LevelFilter;
use core::{
    alloc::{GlobalAlloc, Layout},
    intrinsics::volatile_set_memory,
    panic::PanicInfo,
    ptr::{read_volatile, write_volatile, NonNull},
    sync::atomic::{AtomicU64, Ordering},
};
use crossbeam::queue::ArrayQueue;

mod framebuffer;
use framebuffer::FB;

use conquer_once::spin::OnceCell;
use hashbrown::HashMap;
use x86_64::{
    instructions::hlt,
    structures::paging::{
        mapper::MapToError, FrameAllocator, Mapper, OffsetPageTable, Page, PageTableFlags,
        PhysFrame, Size1GiB, Size2MiB, Size4KiB,
    },
    PhysAddr, VirtAddr,
};
use xmas_elf::{header::Type, program, sections::SectionData, ElfFile};
mod allocator;
mod app;
mod drivers;
mod gdt;
mod globals;
mod interrupts;
mod ioapic;
mod local_apic;
mod logger;
mod memory;
mod pci;
mod serial;
mod task;
mod virtio;
/// This function is called on panic.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    log::error!("{:?}", info);
    loop {}
}
use bootloader_api::config::{BootloaderConfig, Mapping};

use crate::{
    allocator::{AllocFromCtx, ALLOCATOR},
    framebuffer::FBShare,
    interrupts::{a_sleep, global_time_ms, wait_block, Timer, TIME_MS},
    logger::init_logger,
    memory::BootInfoFrameAllocator,
    pci::Bar,
    task::{
        executor::{qpush, yield_once},
        Task,
    },
    virtio::{DeviceType, Virtio},
};

extern "C" fn log_fn(s: *const u8, l: u32) {
    unsafe {
        let slice = core::slice::from_raw_parts(s, l as usize);
        let str_slice = core::str::from_utf8_unchecked(slice);
        log::info!("{}", str_slice)
    }
}

extern "C" fn calloc(size: usize, align: usize) -> *mut u8 {
    // log::info!("alloc {} {}", size, align);
    unsafe { ALLOCATOR.alloc(core::alloc::Layout::from_size_align(size, align).unwrap()) }
}
extern "C" fn cdalloc(ptr: *mut u8, size: usize, align: usize) {
    // log::info!("dealloc {:?} {} {}", ptr, size, align);
    unsafe {
        ALLOCATOR.dealloc(
            ptr,
            core::alloc::Layout::from_size_align(size, align).unwrap(),
        );
    };
}

use acpi::AcpiTables;
#[derive(Clone)]
struct AcpiHandlerImpl;
impl AcpiHandler for AcpiHandlerImpl {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        let s = (size / 4096 + 1) * 4096;
        PhysicalMapping::new(
            physical_address,
            NonNull::new(phys_to_virt(PhysAddr::new(physical_address as u64)).as_mut_ptr())
                .unwrap(),
            s,
            s,
            self.clone(),
        )
    }
    fn unmap_physical_region<T>(region: &PhysicalMapping<Self, T>) {}
}
const ACPI_HANDLER: AcpiHandlerImpl = AcpiHandlerImpl;

use spin::Mutex;
pub static MAPPER: OnceCell<Mutex<OffsetPageTable>> = OnceCell::uninit();
pub static FRAME_ALLOCATOR: OnceCell<Mutex<BootInfoFrameAllocator>> = OnceCell::uninit();

pub static mut VIRTUAL_MAPPING_OFFSET: VirtAddr = VirtAddr::new_truncate(0);
pub fn phys_to_virt(addr: PhysAddr) -> VirtAddr {
    unsafe { VIRTUAL_MAPPING_OFFSET + addr.as_u64() }
}
static OTHER_VIRT: AtomicU64 = AtomicU64::new(0x_5000_0000_0000);
pub fn create_virt_from_phys(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    frame: PhysFrame,
) -> Result<Page, MapToError<Size4KiB>> {
    let start = VirtAddr::new(OTHER_VIRT.fetch_add(4096, Ordering::Relaxed) as u64);
    let page = Page::containing_address(start);
    let flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    unsafe { mapper.map_to(page, frame, flags, frame_allocator)?.flush() };
    return Ok(page);
}

pub fn create_identity_virt_from_phys(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<Page, MapToError<Size4KiB>> {
    let frame = frame_allocator.allocate_frame().unwrap();
    let start = VirtAddr::new(frame.start_address().as_u64());
    let page = Page::containing_address(start);
    let flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    unsafe { mapper.map_to(page, frame, flags, frame_allocator)?.flush() };
    return Ok(page);
}

pub fn with_mapper_framealloc<FUNC, R>(f: FUNC) -> R
where
    FUNC: FnOnce(&mut OffsetPageTable, &mut BootInfoFrameAllocator) -> R,
{
    let mut mapper = MAPPER.get().unwrap().lock();
    let mut frame_allocator = FRAME_ALLOCATOR.get().unwrap().lock();
    let mapper = &mut *mapper;
    let frame_allocator = &mut *frame_allocator;
    f(mapper, frame_allocator)
}

pub fn create_identity_virt_from_phys_n(pages: usize) -> Result<Page, MapToError<Size4KiB>> {
    with_mapper_framealloc(|mapper, frame_allocator| {
        let first_frame = frame_allocator.allocate_frame().unwrap();
        log::info!("first_frame {}", first_frame.start_address().as_u64());
        for i in 1..pages {
            let frame = frame_allocator.allocate_frame().unwrap();
            let frame_start = frame.start_address().as_u64();

            // log::info!("{} : {}", i, frame_start);
            if first_frame.start_address().as_u64() + (i as u64) * 4096 != frame_start {
                panic!("create_identity_virt_from_phys_n NON CONTIGUOUS, {}", i)
            }
        }

        for i in 0..pages {
            let addr = first_frame.start_address().as_u64() + (i as u64) * 4096;
            let frame = PhysFrame::containing_address(PhysAddr::new(addr));
            let page = Page::containing_address(VirtAddr::new(addr));
            let flags = PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::USER_ACCESSIBLE;
            unsafe { mapper.map_to(page, frame, flags, frame_allocator)?.flush() };
        }

        return Ok(Page::containing_address(VirtAddr::new(
            first_frame.start_address().as_u64(),
        )));
    })
}

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config.kernel_stack_size = 128 * 1024;
    config
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    gdt::init();
    interrupts::init_idt();

    let framebuffer = boot_info.framebuffer.as_mut().unwrap();
    let fbinfo = framebuffer.info();

    let fbm = framebuffer.buffer_mut();
    let fbm2 = unsafe {
        let p = fbm.as_mut_ptr();
        slice::from_raw_parts_mut(p, fbinfo.byte_len)
    };
    init_logger(fbm, fbinfo.clone(), LevelFilter::Trace, true, true);

    // x86_64::instructions::interrupts::int3();
    let virtual_full_mapping_offset = VirtAddr::new(
        boot_info
            .physical_memory_offset
            .into_option()
            .expect("no physical_memory_offset"),
    );
    log::info!("physical_memory_offset {:x}", virtual_full_mapping_offset);
    unsafe {
        VIRTUAL_MAPPING_OFFSET = virtual_full_mapping_offset;
    }
    let mapper = unsafe { memory::init(virtual_full_mapping_offset) };
    let frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_regions) };
    MAPPER.init_once(|| Mutex::new(mapper));
    FRAME_ALLOCATOR.init_once(|| Mutex::new(frame_allocator));
    {
        log::info!("Complete Bootloader Map physical memory");
        type VirtualMappingPageSize = Size2MiB; // Size2MiB;Size1GiB Size4KiB

        let start_frame: PhysFrame<VirtualMappingPageSize> =
            PhysFrame::containing_address(PhysAddr::new(0));
        let max_phys = PhysAddr::new(virtual_full_mapping_offset.as_u64() - 1u64);
        let max_phys = PhysAddr::new(Size1GiB::SIZE * 64 - 1);

        let end_frame: PhysFrame<VirtualMappingPageSize> = PhysFrame::containing_address(max_phys);

        use x86_64::structures::paging::PageSize;
        let mut news = 0;
        let mut olds = 0;
        for frame in PhysFrame::range_inclusive(start_frame, end_frame) {
            let page: Page<VirtualMappingPageSize> = Page::containing_address(
                virtual_full_mapping_offset + frame.start_address().as_u64(),
            );
            let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
            match unsafe {
                MAPPER.get_unchecked().lock().map_to(
                    page,
                    frame,
                    flags,
                    &mut *FRAME_ALLOCATOR.get_unchecked().lock(),
                )
            } {
                Ok(tlb) => {
                    tlb.flush();
                    news += 1;
                }
                Err(_) => {
                    olds += 1;
                }
            };
        }
        log::info!("new:{} already_mapped:{}", news, olds);
    }

    with_mapper_framealloc(|mapper, frame_allocator| {
        allocator::init_heap(mapper, frame_allocator).expect("heap initialization failed");
    });

    let rsdp_addr = boot_info.rsdp_addr.into_option().expect("no rsdp");
    let acpi_tables = unsafe { AcpiTables::from_rsdp(ACPI_HANDLER, rsdp_addr as usize).unwrap() };
    log::info!("acpi_read");

    let x = HpetInfo::new(&acpi_tables).expect("hpet");
    // log::info!("{:#?}]", x);

    let pi = acpi_tables.platform_info().expect("platform info");

    if let InterruptModel::Apic(apic) = pi.interrupt_model {
        // log::info!("{:#?}", apic);

        unsafe {
            log::info!("init apic");
            let lapic = local_apic::LocalApic::init(PhysAddr::new(apic.local_apic_address));
            log::info!("start apic c");
            let mut freq = 1000_000_000;
            if let Some(cpuid) = local_apic::cpuid() {
                log::info!("cpuid");
                if let Some(tsc) = cpuid.get_tsc_info() {
                    log::info!(
                        "{} {}",
                        tsc.nominal_frequency(),
                        tsc.tsc_frequency().unwrap()
                    );
                    freq = tsc.nominal_frequency();
                } else {
                }
            }
            lapic.set_div_conf(0b1011);
            log::info!("start apic c");
            lapic.set_lvt_timer((1 << 17) + 48);
            let wanted_freq_hz = 1000;
            lapic.set_init_count(freq / wanted_freq_hz);
        }

        for io_apic in apic.io_apics.iter() {
            log::info!("{:x}", io_apic.address);
            let mut ioa = ioapic::IoApic::init(io_apic);
            let val = ioa.read(ioapic::IOAPICVER);
            log::info!("{:x}", val);
            for i in 0..24 {
                let n = ioa.read_redtlb(i);
                let mut red = ioapic::RedTbl::new(n);
                red.vector = (50 + i) as u8;

                let stored = red.store();

                ioa.write_redtlb(i, stored);
            }
        }

        x86_64::instructions::interrupts::enable();

        // x86_64::instructions::interrupts::disable();
    }

    {
        // aml::AmlContext::new()
    }
    // .expect("no acpi table");

    let proc_info = pi.processor_info.expect("processor_info");
    // log::info!("{:?}", pi.power_profile);
    // log::info!("{:#?}", pi.interrupt_model);
    // log::info!("{:?}", proc_info.boot_processor);
    for proc in proc_info.application_processors.iter() {
        log::info!("{:?}", proc);
    }

    // for ent in mapper.level_4_table().iter().take(30) {
    //     log::info!("{:?}", ent);
    // }

    let pcis = pci::Pcis::new();

    let mut virtio_devices = Vec::new();

    {
        for (pci_index, pci) in pcis.devs.iter().enumerate() {
            let vector_base = 50 + 2 * pci_index;
            let status = pci.config_read_u16(pci::PCIConfigRegisters::PCIStatus as u8);
            let vendor = pci.config_read_u16(pci::PCIConfigRegisters::PCIVendorID as u8);
            let device_id =
                pci.config_read_u16(pci::PCIConfigRegisters::PCIDeviceID as u8) as isize - 0x1040;
            log::info!(
                "{:?} status {} irq:{} ipin:{}, {:x} {} ________________",
                pci,
                status,
                pci.get_irq(),
                pci.get_ipin(),
                vendor,
                device_id,
            );
            const VIRTIO_VENDOR_ID: u16 = 0x1af4;
            if vendor == VIRTIO_VENDOR_ID {
                let virtio = with_mapper_framealloc(|mapper, frame_allocator| {
                    Virtio::init(pci, mapper, frame_allocator)
                });
                if let Some(virtio) = virtio {
                    virtio_devices.push(virtio);
                }
            }
        }
    }

    let mut fb = Box::new(FB::new(&fbinfo));
    // fb.flush(fbm2, &fbinfo);
    let fb_clone: *mut FB = &mut *fb;
    log::info!("fbclone {:?}", fb_clone);

    {
        let mut executor = task::executor::Executor::new();
        let spawner = executor.spawner();

        for virtio in virtio_devices.into_iter() {
            match virtio.device_type {
                DeviceType::Input => spawner.run(drivers::virtio_input::drive(virtio)),
                DeviceType::Gpu => spawner.run(drivers::virtio_gpu::drive(
                    virtio,
                    spawner.clone(),
                    fb_clone,
                )),
            }
        }

        spawner.run(async move {
            use app::*;
            let mut apps: Vec<App> = Vec::new();
            let apps_raw = [
                &include_bytes!("../../../app_background/target/x86_64/release/func")[..],
                &include_bytes!("../../../app_console/target/x86_64/release/func")[..],
                &include_bytes!("../../../app_cursor/target/x86_64/release/func")[..],
                // &include_bytes!("../../../app_test/target/x86_64/release/func")[..],
                // &include_bytes!("../../../app_c/target/main")[..],
            ];
            for app_bytes in apps_raw.iter() {
                apps.push(App::new(app_bytes, false));
            }

            loop {
                let input = globals::INPUT.read();
                for app in apps.iter_mut() {
                    let mut arg = Context::new(log_fn, fb.share(), calloc, cdalloc, &input);
                    app.call(&mut arg);
                }

                globals::INPUT.update(|e| e.step());
                yield_once().await;
            }
        });
        executor.run();
    }
}
