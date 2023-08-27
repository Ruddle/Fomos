#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    //  x86_64::instructions::interrupts::int3();
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

pub const HISTORY_SIZE: usize = 64;

#[repr(usize)]
#[derive(Clone, Debug, Copy, PartialEq)]
pub enum Key {
    Reserved = 0,
    Esc = 1,
    Key1 = 2,
    Key2 = 3,
    Key3 = 4,
    Key4 = 5,
    Key5 = 6,
    Key6 = 7,
    Key7 = 8,
    Key8 = 9,
    Key9 = 10,
    Key0 = 11,
    KeyMinus = 12,
    KeyEqual = 13,
    KeyBackspace = 14,
    KeyTab = 15,
    KeyQ = 16,
    KeyW = 17,
    KeyE = 18,
    KeyR = 19,
    KeyT = 20,
    KeyY = 21,
    KeyU = 22,
    KeyI = 23,
    KeyO = 24,
    KeyP = 25,
    KeyLeftBrace = 26,
    KeyRightBrace = 27,
    KeyEnter = 28,
    KeyLeftCtrl = 29,
    KeyA = 30,
    KeyS = 31,
    KeyD = 32,
    KeyF = 33,
    KeyG = 34,
    KeyH = 35,
    KeyJ = 36,
    KeyK = 37,
    KeyL = 38,
    KeySemicolon = 39,
    KeyApostrophe = 40,
    KeyGrave = 41,
    KeyLeftShift = 42,
    KeyBackslash = 43,
    KeyZ = 44,
    KeyX = 45,
    KeyC = 46,
    KeyV = 47,
    KeyB = 48,
    KeyN = 49,
    KeyM = 50,
    KeyComma = 51,
    KeyDot = 52,
    KeySlash = 53,
    KeyRightShift = 54,
    KeyKpAsterisk = 55,
    KeyLeftAlt = 56,
    KeySpace = 57,
    KeyCapsLock = 58,
    KeyF1 = 59,
    KeyF2 = 60,
    KeyF3 = 61,
    KeyF4 = 62,
    KeyF5 = 63,
    KeyF6 = 64,
    KeyF7 = 65,
    KeyF8 = 66,
    KeyF9 = 67,
    KeyF10 = 68,
    KeyNumLock = 69,
    KeyScrollLock = 70,
    KeyKp7 = 71,
    KeyKp8 = 72,
    KeyKp9 = 73,
    KeyKpMinus = 74,
    KeyKp4 = 75,
    KeyKp5 = 76,
    KeyKp6 = 77,
    KeyKpPlus = 78,
    KeyKp1 = 79,
    KeyKp2 = 80,
    KeyKp3 = 81,
    KeyKp0 = 82,
    KeyKpDot = 83,
    KeyZenkakuHankaku = 85,
    Key102nd = 86,
    KeyF11 = 87,
    KeyF12 = 88,
    KeyRo = 89,
    KeyKatakana = 90,
    KeyHiragana = 91,
    KeyHenkan = 92,
    KeyKatakanaHiragana = 93,
    KeyMuhenkan = 94,
    KeyKpJpComma = 95,
    KeyKpEnter = 96,
    KeyRightCtrl = 97,
    KeyKpSlash = 98,
    KeySysRq = 99,
    KeyRightAlt = 100,
    KeyLineFeed = 101,
    KeyHome = 102,
    KeyUp = 103,
    KeyPageUp = 104,
    KeyLeft = 105,
    KeyRight = 106,
    KeyEnd = 107,
    KeyDown = 108,
    KeyPageDown = 109,
    KeyInsert = 110,
    KeyDelete = 111,
    KeyMacro = 112,
    KeyMute = 113,
    KeyVolumeDown = 114,
    KeyVolumeUp = 115,
    KeyPower = 116,
    KeyKpEqual = 117,
    KeyKpPlusMinus = 118,
    KeyPause = 119,
    KeyScale = 120,
    KeyKpComma = 121,
    KeyHangeul = 122,
    KeyHanja = 123,
    KeyYen = 124,
    KeyLeftMeta = 125,
    KeyRightMeta = 126,
    KeyCompose = 127,
    KeyStop = 128,
    KeyAgain = 129,
    KeyProps = 130,
    KeyUndo = 131,
    KeyFront = 132,
    KeyCopy = 133,
    KeyOpen = 134,
    KeyPaste = 135,
    KeyFind = 136,
    KeyCut = 137,
    KeyHelp = 138,
    KeyMenu = 139,
    KeyCalc = 140,
    KeySetup = 141,
    KeySleep = 142,
    KeyWakeup = 143,
    KeyFile = 144,
    KeySend = 145,
    KeyDeleteFile = 146,
    KeyXfer = 147,
    KeyProg1 = 148,
    KeyProg2 = 149,
    KeyWww = 150,
    KeyMsDos = 151,
    // KeyCoffee = 152,
    KeyScreenLock = 152,
    KeyDirection = 153,
    KeyCycleWindows = 154,
    KeyMail = 155,
    KeyBookmarks = 156,
    KeyComputer = 157,
    KeyBack = 158,
    KeyForward = 159,
    KeyCloseCd = 160,
    KeyEjectCd = 161,
    KeyEjectCloseCd = 162,
    KeyNextSong = 163,
    KeyPlayPause = 164,
    KeyPreviousSong = 165,
    KeyStopCd = 166,
    KeyRecord = 167,
    KeyRewind = 168,
    KeyPhone = 169,
    KeyIso = 170,
    KeyConfig = 171,
    KeyHomePage = 172,
    KeyRefresh = 173,
    KeyExit = 174,
    KeyMove = 175,
    KeyEdit = 176,
    KeyScrollUp = 177,
    KeyScrollDown = 178,
    KeyKpLeftParenthesis = 179,
    KeyKpRightParenthesis = 180,
    KeyNew = 181,
    KeyRedo = 182,
    KeyF13 = 183,
    KeyF14 = 184,
    KeyF15 = 185,
    KeyF16 = 186,
    KeyF17 = 187,
    KeyF18 = 188,
    KeyF19 = 189,
    KeyF20 = 190,
    KeyF21 = 191,
    KeyF22 = 192,
    KeyF23 = 193,
    KeyF24 = 194,
    KeyMax = 195,

    BtnLeft = 0x110,
    BtnRight = 0x111,
    BtnMiddle = 0x112,
    BtnSide = 0x113,
}

impl Key {
    pub fn char(&self) -> Option<char> {
        match self {
            Key::KeyA => Some('a'),
            Key::KeyB => Some('b'),
            Key::KeyC => Some('c'),
            Key::KeyD => Some('d'),
            Key::KeyE => Some('e'),
            Key::KeyF => Some('f'),
            Key::KeyG => Some('g'),
            Key::KeyH => Some('h'),
            Key::KeyI => Some('i'),
            Key::KeyJ => Some('j'),
            Key::KeyK => Some('k'),
            Key::KeyL => Some('l'),
            Key::KeyM => Some('m'),
            Key::KeyN => Some('n'),
            Key::KeyO => Some('o'),
            Key::KeyP => Some('p'),
            Key::KeyQ => Some('q'),
            Key::KeyR => Some('r'),
            Key::KeyS => Some('s'),
            Key::KeyT => Some('t'),
            Key::KeyU => Some('u'),
            Key::KeyV => Some('v'),
            Key::KeyW => Some('w'),
            Key::KeyX => Some('x'),
            Key::KeyY => Some('y'),
            Key::KeyZ => Some('z'),
            Key::Key0 => Some('0'),
            Key::Key1 => Some('1'),
            Key::Key2 => Some('2'),
            Key::Key3 => Some('3'),
            Key::Key4 => Some('4'),
            Key::Key5 => Some('5'),
            Key::Key6 => Some('6'),
            Key::Key7 => Some('7'),
            Key::Key8 => Some('8'),
            Key::Key9 => Some('9'),
            Key::KeyMinus => Some('-'),
            Key::KeyLeftBrace => Some('{'),
            Key::KeyRightBrace => Some('}'),
            Key::KeySemicolon => Some(';'),
            Key::KeyComma => Some(','),
            Key::KeySlash => Some('/'),
            Key::KeyBackslash => Some('\\'),
            Key::KeyEnter => Some('\n'),
            Key::KeySpace => Some(' '),
            _ => None,
        }
    }
}

#[repr(C)]
#[derive(Clone, Debug, Copy)]
pub struct InputEvent {
    pub trigger: bool,
    pub key: Key,
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
