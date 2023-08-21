#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;
mod st;
use alloc::boxed::Box;
use st::*;
use vek::Vec2;

pub struct Store {
    x: usize,
    y: usize,
    xm: usize,
    ym: usize,
}

#[no_mangle]
pub extern "C" fn _start(ctx: &mut Context<Store>) -> i32 {
    unsafe { ALLOCATOR.swap(ctx) };
    unsafe { LOGGER.swap(ctx.log) };

    // (ctx.log)("back start");
    // x86_64::instructions::interrupts::int3();

    let mut store: Box<Store>;
    if let Some(ptr) = ctx.store.take() {
        // st::log("store found");
        store = ptr;
    } else {
        st::log("store not found");

        store = Box::new(Store {
            x: ctx.input.mx,
            y: ctx.input.my,
            xm: ctx.input.mx,
            ym: ctx.input.my,
        })
    }

    let am = Vec2::new(store.xm as f32 + 0.01, store.ym as f32);
    let a = Vec2::new(store.x as f32, store.y as f32);
    let b = Vec2::new(ctx.input.mx as f32 + 0.01, ctx.input.my as f32 + 0.01);

    fn cro(a: Vec2<f32>, b: Vec2<f32>) -> f32 {
        a.x * b.y - a.y * b.x
    }

    fn sd_bezier(p: Vec2<f32>, v0: Vec2<f32>, v1: Vec2<f32>, v2: Vec2<f32>) -> f32 {
        let mid = (v0 + v2) * 0.5;
        let to_v1 = v1 - mid;
        let v1 = v1 + to_v1 * 1.;

        let i = v0 - v2;
        let j = v2 - v1;
        let k = v1 - v0;
        let w = j - k;

        let v0 = v0 - p;
        let v1 = v1 - p;
        let v2 = v2 - p;

        let x = cro(v0, v2);
        let y = cro(v1, v0);
        let z = cro(v2, v1);

        let s = 2.0 * (y * j + z * k) - x * i;

        let r = (y * z - x * x * 0.25) / s.dot(s);
        let t = ((0.5 * x + y + r * s.dot(w)) / (x + y + z)).clamp(0.0, 1.0);

        (v0 + t * (k + k + t * w)).magnitude()
    }

    for y in 0..ctx.fb.h {
        for x in 0..ctx.fb.w {
            let p = &mut ctx.fb.pixels[x + y * ctx.fb.w];
            if (x as i32 - ctx.input.mx as i32).abs() + (y as i32 - ctx.input.my as i32).abs() < 80
            {
                let pos = Vec2::new(x as f32, y as f32);
                let d = sd_bezier(pos, am, a, b);
                if d < 3. {
                    p.r = 255;

                    let left_click = ctx.input.keys[0x110];
                    if left_click < 128 {
                        p.g = 255;
                        p.b = 255;
                    } else if left_click == 128 {
                        p.g = 255;
                        p.b = 0;
                    } else {
                        p.g = 0;
                        p.b = 0;
                    }
                } else if d < 4. {
                    p.r = 0;
                    p.g = 0;
                    p.b = 0;
                }
            }
        }
    }
    store.xm = store.x;
    store.ym = store.y;
    store.x = ctx.input.mx;
    store.y = ctx.input.my;
    *ctx.store = Some(store);

    return 0;
}
