use bootloader_api::info::{FrameBufferInfo, PixelFormat};

use core::{fmt, ptr};
use font_constants::BACKUP_CHAR;
use noto_sans_mono_bitmap::{
    get_raster, get_raster_width, FontWeight, RasterHeight, RasterizedChar,
};

/// Additional vertical space between lines
const LINE_SPACING: usize = 2;
/// Additional horizontal space between characters.
const LETTER_SPACING: usize = 0;

/// Padding from the border. Prevent that font is too close to border.
const BORDER_PADDING: usize = 1;

/// Constants for the usage of the [`noto_sans_mono_bitmap`] crate.
mod font_constants {
    use super::*;

    /// Height of each char raster. The font size is ~0.84% of this. Thus, this is the line height that
    /// enables multiple characters to be side-by-side and appear optically in one line in a natural way.
    pub const CHAR_RASTER_HEIGHT: RasterHeight = RasterHeight::Size16;

    /// The width of each single symbol of the mono space font.
    pub const CHAR_RASTER_WIDTH: usize = get_raster_width(FontWeight::Regular, CHAR_RASTER_HEIGHT);

    /// Backup character if a desired symbol is not available by the font.
    /// The '�' character requires the feature "unicode-specials".
    pub const BACKUP_CHAR: char = '�';

    pub const FONT_WEIGHT: FontWeight = FontWeight::Regular;
}

/// Returns the raster of the given char or the raster of [`font_constants::BACKUP_CHAR`].
fn get_char_raster(c: char) -> RasterizedChar {
    fn get(c: char) -> Option<RasterizedChar> {
        get_raster(
            c,
            font_constants::FONT_WEIGHT,
            font_constants::CHAR_RASTER_HEIGHT,
        )
    }
    get(c).unwrap_or_else(|| get(BACKUP_CHAR).expect("Should get raster of backup char."))
}

/// Allows logging text to a pixel-based framebuffer.
pub struct FrameBufferWriter {
    framebuffer: &'static mut [u8],
    info: FrameBufferInfo,
    x_pos: usize,
    y_pos: usize,
    pub level: usize,
}

impl FrameBufferWriter {
    /// Creates a new logger that uses the given framebuffer.
    pub fn new(framebuffer: &'static mut [u8], info: FrameBufferInfo) -> Self {
        let mut logger = Self {
            framebuffer,
            info,
            x_pos: 0,
            y_pos: 0,
            level: 0,
        };
        logger.clear();
        logger
    }

    fn newline(&mut self) {
        self.y_pos += font_constants::CHAR_RASTER_HEIGHT.val() + LINE_SPACING;
        self.carriage_return()
    }

    fn carriage_return(&mut self) {
        self.x_pos = BORDER_PADDING;
    }

    /// Erases all text on the screen. Resets `self.x_pos` and `self.y_pos`.
    pub fn clear(&mut self) {
        self.x_pos = BORDER_PADDING;
        self.y_pos = BORDER_PADDING;
        self.framebuffer.fill(0);
    }

    fn width(&self) -> usize {
        self.info.width
    }

    fn height(&self) -> usize {
        self.info.height
    }

    /// Writes a single char to the framebuffer. Takes care of special control characters, such as
    /// newlines and carriage returns.
    fn write_char(&mut self, c: char) {
        match c {
            '\n' => self.newline(),
            '\r' => self.carriage_return(),
            c => {
                let new_xpos = self.x_pos + font_constants::CHAR_RASTER_WIDTH;
                if new_xpos >= self.width() {
                    self.newline();
                }
                let new_ypos =
                    self.y_pos + font_constants::CHAR_RASTER_HEIGHT.val() + BORDER_PADDING;
                if new_ypos >= self.height() {
                    self.clear();
                }
                self.write_rendered_char(get_char_raster(c));
            }
        }
    }

    /// Prints a rendered char into the framebuffer.
    /// Updates `self.x_pos`.
    fn write_rendered_char(&mut self, rendered_char: RasterizedChar) {
        for (y, row) in rendered_char.raster().iter().enumerate() {
            for (x, byte) in row.iter().enumerate() {
                self.write_pixel(self.x_pos + x, self.y_pos + y, *byte);
            }
        }
        self.x_pos += rendered_char.width() + LETTER_SPACING;
    }

    fn write_pixel(&mut self, x: usize, y: usize, intensity: u8) {
        let pixel_offset = y * self.info.stride + x;

        let r = intensity;
        let g = [intensity, 0][self.level];
        let b = [intensity / 2, 0][self.level];
        let color = match self.info.pixel_format {
            PixelFormat::Rgb => [r, g, b, 0],
            PixelFormat::Bgr => [b, g, r, 0],
            PixelFormat::U8 => [if intensity > 200 { 0xf } else { 0 }, 0, 0, 0],
            other => {
                // set a supported (but invalid) pixel format before panicking to avoid a double
                // panic; it might not be readable though
                self.info.pixel_format = PixelFormat::Rgb;
                panic!("pixel format {:?} not supported in logger", other)
            }
        };
        let bytes_per_pixel = self.info.bytes_per_pixel;
        let byte_offset = pixel_offset * bytes_per_pixel;
        self.framebuffer[byte_offset..(byte_offset + bytes_per_pixel)]
            .copy_from_slice(&color[..bytes_per_pixel]);
        let _ = unsafe { ptr::read_volatile(&self.framebuffer[byte_offset]) };
    }
}

unsafe impl Send for FrameBufferWriter {}
unsafe impl Sync for FrameBufferWriter {}

impl fmt::Write for FrameBufferWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // for c in s.chars() {
        //     self.write_char(c);
        // }

        Ok(())
    }
}
use alloc::{slice, vec::Vec};

use crate::interrupts::global_time_ms;
// extern crate alloc;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct RGBA {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}
#[derive(Clone)]
#[repr(C)]
pub struct FB {
    pub pixels: Vec<RGBA>,
    pub backbuffer: Vec<RGBA>,
    pub w: usize,
    pub h: usize,
}

#[repr(C)]
pub struct FBShare<'a> {
    pub pixels: &'a mut [RGBA],
    pub w: usize,
    pub h: usize,
}
impl FB {
    pub fn new(info: &FrameBufferInfo) -> Self {
        let w = info.width;
        let h = info.height;
        let mut pixels = Vec::with_capacity(w * h);
        let mut backbuffer = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                pixels.push(RGBA {
                    r: x as u8,
                    g: y as u8,
                    b: 0,
                    a: 0,
                });
                backbuffer.push(pixels[pixels.len() - 1]);
            }
        }
        FB {
            pixels,
            w,
            h,
            backbuffer,
        }
    }

    pub fn update(&mut self, vec: *mut RGBA, w: usize, h: usize) {
        self.pixels = unsafe { Vec::from_raw_parts(vec, h * w, w * h) };
        self.w = w;
        self.h = h;
    }

    pub fn share(&mut self) -> FBShare {
        FBShare {
            pixels: &mut self.pixels[..],
            w: self.w,
            h: self.h,
        }
    }

    pub fn flush(&mut self, framebuffer: &mut [u8], info: &FrameBufferInfo) {
        let mut todraw = &self.pixels;

        let start = global_time_ms();

        // match info.pixel_format {
        //     PixelFormat::Bgr => {
        //         for (idx, &i) in self.pixels.iter().enumerate() {
        //             self.backbuffer[idx].r = i.b;
        //             self.backbuffer[idx].g = i.g;
        //             self.backbuffer[idx].b = i.r;
        //         }
        //         // todraw = &self.backbuffer;
        //     }
        //     _ => {}
        // }

        // let time0 = get_time_ms() - start;

        // let start = get_time_ms();
        framebuffer.copy_from_slice(unsafe {
            slice::from_raw_parts(todraw.as_ptr() as *const u8, framebuffer.len())
        });
        // log::info!("step 0 FB {}ms", time0);
        // log::info!("step 1 FB {}ms", get_time_ms() - start);
        // for y in 0..self.h {
        //     for x in 0..self.w {
        //         let RGBA { r, g, b, a } = self.pixels[x + self.w * y];
        //         let pixel_offset = y * info.stride + x;

        //         let color = match info.pixel_format {
        //             PixelFormat::Rgb => [r, g, b, 0],
        //             PixelFormat::Bgr => [b, g, r, 0],
        //             PixelFormat::U8 => [if (g + b + r) as usize > 200 { 0xf } else { 0 }, 0, 0, 0],
        //             other => {
        //                 // set a supported (but invalid) pixel format before panicking to avoid a double
        //                 // panic; it might not be readable though
        //                 // info.pixel_format = PixelFormat::Rgb;
        //                 panic!("pixel format {:?} not supported in logger", other)
        //             }
        //         };
        //         let bytes_per_pixel = info.bytes_per_pixel;
        //         let byte_offset = pixel_offset * bytes_per_pixel;
        //         framebuffer[byte_offset..(byte_offset + bytes_per_pixel)]
        //             .copy_from_slice(&color[..bytes_per_pixel]);
        //     }
        // }
    }

    // pub fn set(x: usize, y: usize)
}
