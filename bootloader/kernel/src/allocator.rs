use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
extern "C" fn a_init(l: alloc::alloc::Layout) -> *mut u8 {
    panic!("")
}
extern "C" fn d_init(ptr: *mut u8, layout: alloc::alloc::Layout) {
    panic!("")
}

#[repr(C)]
pub struct AllocFromCtx {
    a: extern "C" fn(alloc::alloc::Layout) -> *mut u8,
    d: extern "C" fn(*mut u8, alloc::alloc::Layout),
}
impl AllocFromCtx {
    pub fn init() -> Self {
        Self {
            a: a_init,
            d: d_init,
        }
    }
    pub fn new(
        a: extern "C" fn(alloc::alloc::Layout) -> *mut u8,
        d: extern "C" fn(*mut u8, alloc::alloc::Layout),
    ) -> Self {
        Self { a, d }
    }
}
unsafe impl GlobalAlloc for AllocFromCtx {
    unsafe fn alloc(&self, layout: alloc::alloc::Layout) -> *mut u8 {
        (self.a)(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: alloc::alloc::Layout) {
        (self.d)(ptr, layout)
    }
}
// #[global_allocator]
// static ALLOCATOR: AllocFromCtx = AllocFromCtx;

use linked_list_allocator::LockedHeap;

#[global_allocator]
pub static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 128 * 1024 * 1024; // 100 KiB

use x86_64::{
    structures::paging::{
        mapper::MapToError, FrameAllocator, Mapper, Page, PageTableFlags, PhysFrame, Size4KiB,
    },
    VirtAddr,
};

pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        let flags =
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
        unsafe { mapper.map_to(page, frame, flags, frame_allocator)?.flush() };
    }

    unsafe {
        ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE);
    }

    Ok(())
}
