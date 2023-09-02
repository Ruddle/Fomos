#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(option_get_or_insert_default)]
extern crate alloc;
mod st;

use st::*;

use alloc::{boxed::Box, vec::Vec};

use vek::{num_traits::Zero, Vec3};

use vek::num_traits::Float;

use taffy::prelude::*;

pub struct ConsoleHistory {
    atoms: alloc::vec::Vec<Atom>,
}
impl ConsoleHistory {
    pub fn new() -> ConsoleHistory {
        ConsoleHistory { atoms: Vec::new() }
    }
}

pub struct Atom {
    is_user: bool,
    text: alloc::string::String,
}

enum Mode {
    Shell,
    FomoscriptREPL,
}

pub struct Store {
    x: usize,
    y: usize,
    x2: usize,
    y2: usize,
    resizing: [bool; 4],
    moving: Option<(usize, usize)>,
    b1: Vec<RGBA>,
    b2: Vec<RGBA>,
    step: usize,
    taffy: Taffy,
    input_history_last_index: usize,
    console_history: ConsoleHistory,
    active: bool,
    local: Local,
    shift: bool,
    altg: bool,
    script_ctx: fomoscript::Ctx,
    mode: Mode,
}

fn put(pixel: &mut RGBA, v: Vec3<f32>) {
    pixel.r = v.x as u8;
    pixel.g = v.y as u8;
    pixel.b = v.z as u8;
}

fn get(mut x: isize, mut y: isize, src: &[RGBA], wi: isize, hi: isize) -> Vec3<f32> {
    if x < 0 {
        x = 0;
    } else if x >= wi - 1 {
        x = wi - 1;
    }
    if y < 0 {
        y = 0;
    } else if y >= hi - 1 {
        y = hi - 1;
    }

    let (x, y) = (x as usize, y as usize);
    let index = x + y * wi as usize;
    // st::log(&format!("{} {} {} {}", x, y, wi, hi));
    let r = src[index];
    Vec3::new(r.r as f32, r.g as f32, r.b as f32)
}

fn getf(mut x: f32, mut y: f32, src: &[RGBA], wi: isize, hi: isize) -> RGBA {
    let x0 = x as isize;
    let x1 = x0 + 1;
    let y0 = y as isize;
    let y1 = y0 + 1;

    let fx = x - x0 as f32;
    let fy = y - y0 as f32;

    let a = get(x0, y0, src, wi, hi);
    let b = get(x1, y0, src, wi, hi);
    let c = get(x1, y1, src, wi, hi);
    let d = get(x0, y1, src, wi, hi);

    let ax = (1.0 - fx) * a + fx * b;
    let ay = (1.0 - fx) * d + fx * c;

    let avg = ax * (1.0 - fy) + ay * fy;
    RGBA {
        r: avg.x as u8,
        g: avg.y as u8,
        b: avg.z as u8,
        a: 0,
    }
}

fn pt_in_rect(px: usize, py: usize, x: usize, y: usize, x2: usize, y2: usize) -> bool {
    px >= x && px <= x2 && py >= y && py <= y2
}

fn kernel<const S: usize>(sigma: f32) -> [f32; S] {
    let mut kernel = [0.0; S];
    let mid = S / 2;

    // calculate the Gaussian distribution
    let variance = sigma.powi(2);
    let factor = 1.0 / (2.0 * 3.141592 * variance);
    let mut sum = 0.0;
    for i in 0..S {
        let x = (i as i32 - mid as i32) as f32;
        let value = factor * (-x.powi(2) / (2.0 * variance)).exp();
        kernel[i] = value;
        sum += value;
    }
    sum *= 0.997;
    // normalize the kernel
    for i in 0..S {
        kernel[i] /= sum;
    }

    kernel
}
fn blur(
    fx: isize,
    fy: isize,
    src: &[RGBA],
    dst: &mut Vec<RGBA>,
    wi: isize,
    hi: isize,
    boxes: &[f32; KSIZE],
) {
    let mut avg: Vec3<f32> = Vec3::zero();

    for y in 0..hi {
        for x in 0..wi {
            avg.set_zero();
            for (k, &coef) in boxes.iter().enumerate() {
                let dk = k as isize - KH;
                let v = get(x + fx * dk, y + fy * dk, src, wi, hi);
                avg += v * coef * 0.99;
            }
            put(&mut dst[(x + y * wi) as usize], avg);
        }
    }
}

fn hash(n: usize) -> usize {
    // integer hash copied from Hugo Elias
    let n = (n << 13) ^ n;
    let n = n.wrapping_mul(n.wrapping_mul(n).wrapping_mul(15731) + 789221) + 1376312589;
    n
}

fn hash2(x: usize) -> usize {
    let mut x = ((x >> 16) ^ x) * 0x45d9f3b;
    x = ((x >> 16) ^ x) * 0x45d9f3b;
    (x >> 16) ^ x
}
const KSIZE: usize = 5;
const KH: isize = (KSIZE / 2) as isize;
const DIV: isize = 4;
const ORANGE: RGBA = RGBA {
    r: 255,
    g: 128,
    b: 0,
    a: 0,
};
const GREY3: RGBA = RGBA {
    r: 150,
    g: 150,
    b: 150,
    a: 0,
};
const GREY2: RGBA = RGBA {
    r: 80,
    g: 80,
    b: 80,
    a: 0,
};
const GREY: RGBA = RGBA {
    r: 50,
    g: 50,
    b: 50,
    a: 0,
};
#[no_mangle]
pub extern "C" fn _start(ctx: &mut Context<Store>) -> i32 {
    unsafe { ALLOCATOR.swap(ctx) };
    unsafe { LOGGER.swap(ctx.log) };

    let hi = ctx.fb.h as isize / DIV;
    let wi = ctx.fb.w as isize / DIV;

    let store = ctx.store.get_or_insert_with(|| {
        Box::new({
            Store {
                x: 100 + ctx.pid as usize * 100,
                y: 100 + ctx.pid as usize * 100,
                x2: 1000 + ctx.pid as usize * 100,
                y2: 700 + ctx.pid as usize * 100,
                resizing: [false; 4],
                b1: Vec::with_capacity((wi * hi) as usize),
                b2: Vec::with_capacity((wi * hi) as usize),

                step: 0,
                moving: None,
                taffy: Taffy::new(),
                input_history_last_index: ctx.input.history_last_index,
                console_history: ConsoleHistory::new(),
                active: false,
                local: Local::En,
                shift: false,
                altg: false,
                script_ctx: fomoscript::Ctx::new(),
                mode: Mode::Shell,
            }
        })
    });

    if store.step == 0 {
        store.console_history.atoms.push(Atom {
            is_user: false,
            text: alloc::format!("Welcome to Fomos !"),
        });
        store.console_history.atoms.push(Atom {
            is_user: true,
            text: alloc::format!(">"),
        });
    }

    store.step += 1;
    let blured1 = &mut store.b1;
    let blured2 = &mut store.b2;

    let (src, dst) = if store.step % 2 == 0 {
        (blured1, blured2)
    } else {
        (blured2, blured1)
    };

    if src.len() == 0 {
        for y in 0..hi {
            for x in 0..wi {
                let v = ctx.fb.pixels[((x * DIV) + (y * DIV) * (wi * DIV)) as usize];
                src.push(v);
                dst.push(v);
            }
        }
    } else {
        for y in 0..hi {
            for x in 0..wi {
                if true {
                    //|| (hash2((x + y * wi) as usize) + store.step) % 2 == 0
                    let v = ctx.fb.pixels[((x * DIV) + (y * DIV) * (wi * DIV)) as usize];
                    let s = src[(x + y * wi) as usize];
                    let a = 0.03;
                    let b = 1.0 - a;
                    let r = (v.r as f32 * a + s.r as f32 * b) as u8;
                    let g = (v.g as f32 * a + s.g as f32 * b) as u8;
                    let b = (v.b as f32 * a + s.b as f32 * b) as u8;

                    src[(x + y * wi) as usize] = RGBA { r, g, b, a: 0 };
                }
            }
        }
    }

    let boxes = kernel::<KSIZE>(1.0);

    let fx = if store.step % 2 == 0 { 1 } else { 0 };
    blur(fx, 1 - fx, src, dst, wi, hi, &boxes);

    // blur(1, 0, &blured2, &mut blured1);
    // for y in 0..hi {
    //     for x in 0..wi {
    //         avg.set_zero();
    //         for (k, coef) in boxes.iter().enumerate() {
    //             let coef = boxes[k as usize];
    //             let v = get(x + (k as isize - KH), y);
    //             avg += v * coef;
    //         }
    //         put(&mut blured1[(x + y * wi) as usize], avg);
    //     }
    // }

    if ctx.input.keys[272] < 128 {
        store.resizing = [false; 4];
        store.moving = None;
    }

    if ctx.input.keys[272] == 128 {
        if (store.x as isize - ctx.input.mx as isize).abs() < 10 {
            store.resizing[3] = true;
        }
        if (store.x2 as isize - ctx.input.mx as isize).abs() < 10 {
            store.resizing[1] = true;
        }
        if (store.y as isize - ctx.input.my as isize).abs() < 10
            && store.x <= ctx.input.mx
            && store.x2 >= ctx.input.mx
        {
            store.resizing[0] = true;
        }
        if (store.y2 as isize - ctx.input.my as isize).abs() < 10
            && store.x <= ctx.input.mx
            && store.x2 >= ctx.input.mx
        {
            store.resizing[2] = true;
        }

        if !store.resizing.iter().any(|&e| e) {
            if store.x <= ctx.input.mx
                && store.x2 >= ctx.input.mx
                && store.y <= ctx.input.my
                && store.y2 >= ctx.input.my
            {
                store.moving = Some((ctx.input.mx, ctx.input.my));
                store.active = true;
            } else {
                store.active = false;
            }
        }
    }

    if let Some((x0, y0)) = store.moving.as_mut() {
        let dx = ctx.input.mx as isize - *x0 as isize;
        let dy = ctx.input.my as isize - *y0 as isize;

        store.x = (store.x as isize + dx) as usize;
        store.x2 = (store.x2 as isize + dx) as usize;
        store.y = (store.y as isize + dy) as usize;
        store.y2 = (store.y2 as isize + dy) as usize;

        *x0 = ctx.input.mx;
        *y0 = ctx.input.my;
    }

    for (index, &r) in store.resizing.iter().enumerate() {
        if r {
            match index {
                0 => store.y = ctx.input.my,
                1 => store.x2 = ctx.input.mx,
                2 => store.y2 = ctx.input.my,
                3 => store.x = ctx.input.mx,
                _ => {}
            }
        }
    }

    if store.x < 0 || store.x > 10000 {
        store.x = 0;
    }
    if store.y < 0 || store.y > 10000 {
        store.y = 0;
    }
    if store.x2 >= ctx.fb.w {
        store.x2 = ctx.fb.w - 1;
    }
    if store.y2 >= ctx.fb.h {
        store.y2 = ctx.fb.h - 1;
    }

    let taffy = &mut store.taffy;
    taffy.clear();

    let header_node = taffy
        .new_leaf(Style {
            size: Size {
                width: percent(1.0),
                height: points(20.0),
            },
            ..Default::default()
        })
        .unwrap();

    let body_node = taffy
        .new_leaf(Style {
            flex_direction: FlexDirection::Column,
            size: Size {
                width: percent(1.0),
                height: auto(),
            },
            // padding: Rect {
            //     left: points(10.),
            //     right: points(10.),
            //     top: points(10.),
            //     bottom: points(10.),
            // },
            flex_grow: 1.0,
            ..Default::default()
        })
        .unwrap();

    for i in 0..0 {
        let text_node = taffy
            .new_leaf(Style {
                size: Size {
                    width: auto(),
                    height: points(10.),
                },
                margin: Rect {
                    left: points(10.),
                    right: points(10.),
                    top: points(10.),
                    bottom: points(10.),
                },
                flex_grow: 1.0,
                ..Default::default()
            })
            .unwrap();

        let _ = taffy.add_child(body_node, text_node);
    }

    let root_node = taffy
        .new_with_children(
            Style {
                flex_direction: FlexDirection::Column,
                size: Size {
                    width: points(store.x2 as f32 - store.x as f32),
                    height: points(store.y2 as f32 - store.y as f32),
                },
                ..Default::default()
            },
            &[header_node, body_node],
        )
        .unwrap();

    // Call compute_layout on the root of your tree to run the layout algorithm
    taffy
        .compute_layout(
            root_node,
            Size {
                width: points(store.x2 as f32 - store.x as f32),
                height: points(store.y2 as f32 - store.y as f32),
            },
        )
        .unwrap();

    let get_rect = |node| {
        let l = taffy.layout(node).unwrap();
        let (x, y) = (l.location.x as usize, l.location.y as usize);
        let (x2, y2) = (x + l.size.width as usize, y + l.size.height as usize);
        (x + store.x, y + store.y, x2 + store.x, y2 + store.y)
    };

    // for i in 0..5 {
    //     let (cx, cy, cx2, cy2) = get_rect(taffy.child_at_index(body_node, i).unwrap());
    //     st::log(&format!("{}: {:?}", i, (cx, cy, cx2, cy2)));
    // }

    for y in store.y..=store.y2 {
        for x in store.x..=store.x2 {
            let p = &mut ctx.fb.pixels[x + y * ctx.fb.w];
            let mut drawn = false;

            // *p = dst[x / div + (y / div) * (ctx.fb.w / div)];

            // if y - store.y < 20 {
            //     *p = GREY;
            // }

            {
                let (cx, cy, cx2, cy2) = get_rect(header_node);

                if pt_in_rect(x, y, cx, cy, cx2, cy2) {
                    *p = if store.active { GREY2 } else { GREY };
                    drawn = true;
                }
            }

            for i in 0..taffy.child_count(body_node).unwrap() {
                let (cx, cy, cx2, cy2) = get_rect(taffy.child_at_index(body_node, i).unwrap());
                if pt_in_rect(x, y, cx, cy, cx2, cy2) {
                    *p = if i % 2 == 0 { GREY3 } else { ORANGE };
                    drawn = true;
                }
            }

            if (x == store.x) || (x == store.x2) || (y == store.y) || (y == store.y2) {
                *p = if store.active { GREY2 } else { GREY };
                if store.moving.is_some() {
                    *p = ORANGE;
                }
                drawn = true;
            }

            if (x == store.x && store.resizing[3])
                || (x == store.x2 && store.resizing[1])
                || (y == store.y && store.resizing[0])
                || (y == store.y2 && store.resizing[2])
            {
                *p = ORANGE;
                drawn = true;
            }

            if !drawn {
                let div = DIV as usize;
                // *p = getf(x as f32 / div as f32, y as f32 / div as f32, dst, wi, hi);
                *p = dst[x / div + (y / div) * (ctx.fb.w / div)];
            }
        }
    }

    //Write window title
    {
        use noto_sans_mono_bitmap::{get_raster, get_raster_width, FontWeight, RasterHeight};
        let s = alloc::format!("app_console [{}]", ctx.pid);
        let mut cursor_x = 0;
        let padding = 2;
        let weight = FontWeight::Regular;
        for c in s.chars() {
            let width = get_raster_width(weight, RasterHeight::Size16);

            let char_raster =
                get_raster(c, weight, RasterHeight::Size16).expect("unsupported char");

            for (row_i, row) in char_raster.raster().iter().enumerate() {
                for (col_i, pixel) in row.iter().enumerate() {
                    let x = store.x + col_i + padding + cursor_x;
                    let y = store.y + row_i + padding + 0;
                    if x <= 0
                        || x >= ctx.fb.w
                        || y <= 0
                        || y >= ctx.fb.h
                        || x >= store.x2
                        || y >= store.y2
                    {
                        continue;
                    }
                    let p = &mut ctx.fb.pixels[x + y * ctx.fb.w];
                    p.r = *pixel.max(&p.r);
                    p.g = *pixel.max(&p.g);
                    p.b = *pixel.max(&p.b);
                }
            }
            cursor_x += width;
        }
    }
    //Write text buffer
    {
        use noto_sans_mono_bitmap::{get_raster, get_raster_width, FontWeight, RasterHeight};

        let mut cursor_x = 0;
        let mut cursor_y = 20;
        let padding = 2;
        let weight = FontWeight::Regular;

        for Atom { is_user, text } in store.console_history.atoms.iter() {
            for c in text.chars() {
                if (c == '\n') {
                    cursor_y += 16;
                    cursor_x = 0;
                    continue;
                }
                let width = get_raster_width(weight, RasterHeight::Size16);

                let char_raster =
                    get_raster(c, weight, RasterHeight::Size16).expect("unsupported char");

                for (row_i, row) in char_raster.raster().iter().enumerate() {
                    for (col_i, pixel) in row.iter().enumerate() {
                        let x = store.x + col_i + padding + cursor_x;
                        let y = store.y + row_i + padding + cursor_y;
                        if x <= 0
                            || x >= ctx.fb.w
                            || y <= 0
                            || y >= ctx.fb.h
                            || x >= store.x2
                            || y >= store.y2
                        {
                            continue;
                        }
                        let p = &mut ctx.fb.pixels[x + y * ctx.fb.w];

                        p.r = *pixel.max(&p.r);
                        p.g = *pixel.max(&p.g);
                        if *is_user {
                            p.b = *pixel.max(&p.b);
                        };
                    }
                }
                cursor_x += width;
            }

            cursor_y += 16;
            cursor_x = 0;
        }
    }

    if store.active {
        'new_inputs: for i in (store.input_history_last_index + 1)..=ctx.input.history_last_index {
            let InputEvent { trigger, key } = ctx.input.history_ring[i % HISTORY_SIZE];

            match key {
                Key::KeyLeftShift => {
                    store.shift = trigger;
                }
                Key::KeyRightAlt => {
                    store.altg = trigger;
                }
                _ => {}
            }

            if (trigger) {
                let last = store.console_history.atoms.last_mut();
                match last {
                    Some(Atom { is_user, text }) if *is_user => {
                        log(&alloc::format!("{:?}", key));

                        match key {
                            Key::KeyEnter if !store.shift => {
                                if let Mode::FomoscriptREPL = store.mode {
                                    use fomoscript::*;
                                    store.script_ctx.insert_code(&text[4..]);
                                    if let Ok(parent) = store.script_ctx.parse_next_expr() {
                                        let res = eval(&parent, &mut store.script_ctx);
                                        store.console_history.atoms.push(Atom {
                                            is_user: false,
                                            text: alloc::format!("eval: {:?}", res),
                                        });
                                    }
                                } else {
                                    match text.as_ref() {
                                        ">repl" => {
                                            use fomoscript::*;
                                            store.mode = Mode::FomoscriptREPL;
                                            store.console_history.atoms.push(Atom {
                                                is_user: false,
                                                text: alloc::format!(
                                                    "Call quit() to exit the REPL"
                                                ),
                                            });

                                            let store_mode_ptr: *mut Mode = &mut store.mode;
                                            let quit = alloc::rc::Rc::new(
                                                move |a: N, b: N, c: N, d: N| -> N {
                                                    let store_mode =
                                                        unsafe { &mut *store_mode_ptr };
                                                    *store_mode = Mode::Shell;
                                                    N::Unit
                                                },
                                            );
                                            store.script_ctx.set_var_absolute(
                                                "quit",
                                                N::FuncNativeDef(Native(quit)),
                                            );
                                        }
                                        ">pid" => {
                                            store.console_history.atoms.push(Atom {
                                                is_user: false,
                                                text: alloc::format!("pid: {}", ctx.pid),
                                            });
                                        }
                                        ">time" => {
                                            store.console_history.atoms.push(Atom {
                                                is_user: false,
                                                text: alloc::format!("time: {} ms", ctx.start_time),
                                            });
                                        }
                                        ">lang en" => {
                                            store.local = Local::En;
                                            store.console_history.atoms.push(Atom {
                                                is_user: false,
                                                text: alloc::format!("ok"),
                                            });
                                        }
                                        ">lang fr" => {
                                            store.local = Local::Fr;
                                            store.console_history.atoms.push(Atom {
                                                is_user: false,
                                                text: alloc::format!("ok"),
                                            });
                                        }
                                        ">reset" => {
                                            drop(store);
                                            let old = ctx.store.take().unwrap();
                                            drop(old);
                                            return 0;
                                        }
                                        ">help" => {
                                            store.console_history.atoms.push(Atom {
                                                is_user: false,
                                                text: alloc::format!(
                                                    "commands:
    - pid       Display the app pid
    - time      Display the kernel time
    - reset     Clear the app memory
    - lang ..   Set key locale (en,fr)
    - eval ..   Eval fomoscript
    - repl      launch fomoscript REPL
    - help      You are here
    "
                                                ),
                                            });
                                        }
                                        _ => {
                                            if text.starts_with(">eval ") {
                                                use crate::alloc::borrow::ToOwned;
                                                use fomoscript::*;

                                                let res = {
                                                    store.script_ctx.insert_code(&text[5..]);
                                                    store.script_ctx.set_var_absolute(
                                                        "time",
                                                        N::Num(ctx.start_time as f64),
                                                    );
                                                    store.script_ctx.set_var_absolute(
                                                        "pid",
                                                        N::Num(ctx.pid as f64),
                                                    );

                                                    {
                                                        //TODO find a safe way to share native function to interpreter
                                                        let ptr: *mut [RGBA] = ctx.fb.pixels;
                                                        let w = ctx.fb.w;
                                                        let draw_pixel = alloc::rc::Rc::new(
                                                            move |a: N, b: N, c: N, d: N| -> N {
                                                                let arr: &mut [RGBA] =
                                                                    unsafe { &mut *ptr };
                                                                let p = &mut arr[a.as_f64()
                                                                    as usize
                                                                    + (b.as_f64() as usize) * w];

                                                                p.r = (c.as_f64() * 255.0) as u8;
                                                                p.g = p.r;
                                                                p.b = p.b;
                                                                N::Unit
                                                            },
                                                        );
                                                        store.script_ctx.set_var_absolute(
                                                            "draw",
                                                            N::FuncNativeDef(Native(draw_pixel)),
                                                        );
                                                    }
                                                    let mut res = N::Unit;
                                                    while let Ok(parent) =
                                                        store.script_ctx.parse_next_expr()
                                                    {
                                                        res = eval(&parent, &mut store.script_ctx);
                                                    }

                                                    log(&alloc::format!("res {:?}", res));

                                                    res
                                                };

                                                store.console_history.atoms.push(Atom {
                                                    is_user: false,
                                                    text: alloc::format!("eval: {:?}", res),
                                                });
                                            } else {
                                                store.console_history.atoms.push(Atom {
                                                    is_user: false,
                                                    text: alloc::format!("unknown command"),
                                                });
                                            }
                                        }
                                    }
                                }

                                store.console_history.atoms.push(Atom {
                                    is_user: true,
                                    text: alloc::string::String::from(
                                        if let Mode::Shell = store.mode {
                                            ">"
                                        } else {
                                            "fos>"
                                        },
                                    ),
                                });
                                break 'new_inputs;
                            }

                            Key::KeyBackspace => {
                                if text.len() > 1 {
                                    text.pop();
                                }
                            }
                            _ => {}
                        }

                        if let Some(c) = key.char(store.local, store.shift, store.altg) {
                            *text = alloc::format!("{}{}", text, c)
                        } else {
                            match key {
                                Key::KeyEnter if store.shift => {
                                    *text = alloc::format!("{}\n", text,)
                                }
                                _ => {}
                            }
                            // store.text_buffer = alloc::format!("{}{:?}", store.text_buffer, key)
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    store.input_history_last_index = ctx.input.history_last_index;

    return 0;
}
