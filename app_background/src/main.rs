#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;
mod st;

use st::*;

use alloc::boxed::Box;
use imagine::pixel_formats::RGBA8888;

const IMG: &[u8] = include_bytes!("./qr.png");

pub struct Store {
    pix: imagine::image::Bitmap<RGBA8888>,
}

#[no_mangle]
pub extern "C" fn _start(ctx: &mut Context<Store>) -> i32 {
    unsafe { ALLOCATOR.swap(ctx) };
    unsafe { LOGGER.swap(ctx.log) };

    // (ctx.log)("back start");
    // x86_64::instructions::interrupts::int3();

    let store: Box<Store>;
    if let Some(ptr) = ctx.store.take() {
        store = ptr;
    } else {
        st::log("store not found");
        let pix = imagine::image::Bitmap::try_from_png_bytes(IMG);
        if pix.is_none() {
            st::log("No pix");
            return -1;
        }
        store = Box::new(Store { pix: pix.unwrap() })
    }

    let pix = &store.pix;

    let dx = (libm::cosf((ctx.start_time as f32) * 0.001) * 2.0) as usize;

    for y in 0..ctx.fb.h {
        for x in 0..ctx.fb.w {
            let lx = x as f32 / (ctx.fb.w) as f32;
            let ly = y as f32 / (ctx.fb.h) as f32;
            let px = (lx * pix.width as f32) as usize;
            let py = (ly * pix.height as f32) as usize;
            let v = pix.pixels[px + py * pix.width as usize];
            let p = &mut ctx.fb.pixels[x + y * ctx.fb.w];
            p.r = v.r;
            p.g = v.g;
            p.b = v.b;
        }
    }

    *ctx.store = Some(store);

    return 0;
}
