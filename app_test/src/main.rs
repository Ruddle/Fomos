#![no_std]
#![no_main]

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

const TEXT: &str = "Writting from pid: ";
#[repr(C)]
pub struct Context<'a> {
    pub version: u8,
    start_time: u64,
    log: extern "C" fn(s: *const u8, l: u32),
    pid: u64,
    fb: FB<'a>,
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

#[no_mangle]
pub extern "C" fn _start(ctx: &mut Context) -> i32 {
    ctx.version += 1;

    let mut txt = [0; TEXT.len()];
    let m = &mut txt;
    m.copy_from_slice(TEXT.as_bytes());
    m[m.len() - 1] = '0' as u8 + ctx.pid as u8;

    let s = unsafe { core::str::from_utf8_unchecked(&txt) };
    (ctx.log)(s.as_ptr(), s.len() as u32);

    //make thinks blue
    // for px in ctx.fb.pixels.iter_mut() {
    //     px.b = 255;
    // }

    return 0;
}
