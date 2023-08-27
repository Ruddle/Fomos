#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    x86_64::instructions::interrupts::int3();
    unsafe {
        (LOGGER.f)("panic".as_ptr(), 5);
    };
    unsafe {
        let s = &format!("{:?}", info);
        (LOGGER.f)(s.as_ptr(), s.len() as u32)
    };
    loop {}
}

pub static mut LOGGER: Logger = Logger::init();

pub fn log(s: &str) {
    unsafe { (LOGGER.f)(s.as_ptr(), s.len() as u32) }
}

type LogFn = extern "C" fn(*const u8, u32);
extern "C" fn nop(s: *const u8, l: u32) {}
pub struct Logger {
    pub f: LogFn,
}
impl Logger {
    pub const fn init() -> Self {
        Self { f: nop }
    }
    pub fn swap(&mut self, f2: LogFn) {
        self.f = f2;
    }
}
#[repr(C)]
pub struct Context<'a, T> {
    pub version: u8,
    pub start_time: u64,
    pub log: extern "C" fn(s: *const u8, l: u32),
    pub pid: u64,
    pub fb: FB<'a>,
    pub calloc: extern "C" fn(usize, usize) -> *mut u8,
    pub cdalloc: extern "C" fn(*mut u8, usize, usize),
    pub store: &'a mut Option<Box<T>>,
    pub input: &'a Input,
}

const HISTORY_SIZE: usize = 64;

#[repr(C)]
#[derive(Clone, Debug, Copy)]
pub struct InputEvent {
    pub trigger: bool,
    pub key: usize,
}
#[repr(C)]
pub struct Input {
    pub mx: usize,
    pub my: usize,
    pub keys: [u8; 1024],
    pub history_last_index: usize,
    pub history_ring: [InputEvent; HISTORY_SIZE],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct RGBA {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[repr(C)]
pub struct FB<'a> {
    pub pixels: &'a mut [RGBA],
    pub w: usize,
    pub h: usize,
}

use core::alloc::GlobalAlloc;

use alloc::{boxed::Box, format};

extern "C" fn a_init(size: usize, align: usize) -> *mut u8 {
    panic!("")
}
extern "C" fn d_init(ptr: *mut u8, size: usize, align: usize) {
    panic!("")
}
#[repr(C)]
pub struct AllocFromCtx {
    a: extern "C" fn(usize, usize) -> *mut u8,
    d: extern "C" fn(*mut u8, usize, usize),
}
unsafe impl GlobalAlloc for AllocFromCtx {
    unsafe fn alloc(&self, layout: alloc::alloc::Layout) -> *mut u8 {
        (self.a)(layout.size(), layout.align())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: alloc::alloc::Layout) {
        (self.d)(ptr, layout.size(), layout.align());
    }
}
impl AllocFromCtx {
    pub const fn init() -> Self {
        Self {
            a: a_init,
            d: d_init,
        }
    }
    pub fn swap<T>(&mut self, ctx: &mut Context<T>) {
        let ptr = self;
        ptr.a = ctx.calloc;
        ptr.d = ctx.cdalloc;
    }
}
#[global_allocator]
pub static mut ALLOCATOR: AllocFromCtx = AllocFromCtx::init();
