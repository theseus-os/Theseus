use super::*;

struct RawVgaBuffer {
    column_position: usize,
    buffer: Unique<[[Volatile<ScreenChar>; BUFFER_WIDTH]; BUFFER_HEIGHT]>,
}
impl RawVgaBuffer {
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            byte => {
                if self.column_position >= BUFFER_WIDTH {
                    self.new_line();
                }
                let row = BUFFER_HEIGHT - 1;
                let col = self.column_position;

                unsafe {
                    self.buffer.as_mut()[row][col].write(
                        ScreenChar {
                            ascii_character: byte,
                            color_code: ColorCode::default(),
                        }
                    );
                }
                self.column_position += 1;
            }
        }
    }

    fn new_line(&mut self) {
        for row in 1..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                let mut buffer = unsafe { self.buffer.as_mut() };
                let character = buffer[row][col].read();
                buffer[row - 1][col].write(character);
            }
        }
        self.clear_row(BUFFER_HEIGHT - 1);
        self.column_position = 0;
    }

    fn clear_row(&mut self, row: usize) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: ColorCode::default()
        };
        for col in 0..BUFFER_WIDTH {
            unsafe { self.buffer.as_mut()[row][col].write(blank); }
        }
    }
}
impl fmt::Write for RawVgaBuffer {
    fn write_str(&mut self, s: &str) -> ::core::fmt::Result {
        
        let ret = serial_port::write_str(s); // mirror to serial port
        
        for byte in s.bytes() {
            self.write_byte(byte)
        }
        ret
    }
}

static EARLY_VGA_WRITER: Mutex<RawVgaBuffer> = Mutex::new(
    RawVgaBuffer {
        column_position: 0,
        buffer: unsafe { Unique::new_unchecked((VGA_BUFFER_PHYSICAL_ADDR + KERNEL_OFFSET) as *mut _) },
    }
);

#[macro_export]
macro_rules! print_early {
    ($($arg:tt)*) => ({
        $crate::raw::print_args_early(format_args!($($arg)*)).unwrap();
    });
}

#[macro_export]
macro_rules! println_early {
    ($fmt:expr) => (print_early!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => (print_early!(concat!($fmt, "\n"), $($arg)*));
}

pub fn print_args_early(args: fmt::Arguments) -> fmt::Result {
    use core::fmt::Write;
    unsafe { EARLY_VGA_WRITER.force_unlock(); }
    EARLY_VGA_WRITER.lock().write_fmt(args)
}
