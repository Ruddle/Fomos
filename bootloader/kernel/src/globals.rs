use crossbeam::atomic::AtomicCell;

pub static INPUT: GLOBAL<Input> = GLOBAL::new(Input::new());
pub struct GLOBAL<T>(AtomicCell<T>);

impl<T: Copy> GLOBAL<T> {
    pub const fn new(t: T) -> Self {
        Self(AtomicCell::new(t))
    }
    ///Might lose data if multiple thread calls it simultaneously
    pub fn update<F>(&self, func: F)
    where
        F: FnOnce(&mut T),
    {
        let mut v = self.0.load();
        func(&mut v);
        self.0.store(v)
    }
    pub fn read(&self) -> T {
        self.0.load()
    }
}
#[derive(Clone, Debug, Copy)]
pub struct Input {
    pub mouse_x: usize,
    pub mouse_y: usize,
    pub keys: [KeyState; 1024],
}

impl Input {
    pub const fn new() -> Self {
        Self {
            mouse_x: 0,
            mouse_y: 0,
            keys: [KeyState::Off; 1024],
        }
    }
    pub fn step(&mut self) {
        for k in self.keys.iter_mut() {
            k.step();
        }
    }
}

#[repr(u8)]
#[derive(Clone, Debug, Copy)]
pub enum KeyState {
    ///Key is not pressed now
    Off = 0,
    ///Key is not pressed now, but was last frame
    OffFromOn = 1,
    ///Key is not pressed now, it was not pressed last frame either, but was during frame (sequence Off -> On -> Off)
    OffTransientOn = 2,
    OnFromOff = 128,
    OnTransientOff = 129,
    On = 130,
}
impl Default for KeyState {
    fn default() -> Self {
        KeyState::Off
    }
}

impl KeyState {
    pub fn handle_incoming_state(&mut self, b: bool) {
        *self = match (*self, b) {
            (KeyState::Off, true) => KeyState::OnFromOff,
            (KeyState::On, false) => KeyState::OffFromOn,
            (KeyState::OffFromOn, true) => KeyState::OnTransientOff,
            (KeyState::OnFromOff, false) => KeyState::OffTransientOn,
            (_, false) => KeyState::Off,
            (_, true) => KeyState::On,
        }
    }
    ///To call every kernel loop
    pub fn step(&mut self) {
        *self = match *self {
            KeyState::OffTransientOn => KeyState::Off,
            KeyState::OffFromOn => KeyState::Off,
            KeyState::OnFromOff => KeyState::On,
            KeyState::OnTransientOff => KeyState::On,
            _ => *self,
        }
    }
}
