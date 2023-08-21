use core::fmt;

pub struct SerialPort {
    port: uart_16550::SerialPort,
}

impl SerialPort {
    /// # Safety
    ///
    /// unsafe because this function must only be called once
    pub unsafe fn init() -> Self {
        let mut port = unsafe { uart_16550::SerialPort::new(0x3F8) };
        port.init();
        Self { port }
    }
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.port.write_str(s).unwrap();
        Ok(())
    }
}
