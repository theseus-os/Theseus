// Adapted from Phillip Opperman's blog os. 

use core::ptr::Unique;
use core::fmt;
use spin::Mutex;
use volatile::Volatile;
use collections::string::String;

const BUFFER_HEIGHT: usize = 25;
const BUFFER_WIDTH: usize = 80;

const VGA_BUFFER_PHYSICAL_ADDR: usize = 0xb8000;

static WRITER: Mutex<Writer> = Mutex::new(Writer {
                    column_position: 0,
                    color_code: ColorCode::new(Color::LightGreen,
                                                Color::Black),
                    buffer: unsafe { Unique::new((VGA_BUFFER_PHYSICAL_ADDR + ::__KERNEL_OFFSET) as *mut _) },
                });


/// This is UNSAFE because it bypasses the VGA Buffer lock. Use print!() instead. 
/// Should only be used in exception contexts and early bring-up code 
/// before the console-based print!() macro is available. 
#[macro_export]
macro_rules! print_unsafe {
    ($($arg:tt)*) => ({
            $crate::drivers::vga_buffer::print_args_unsafe(format_args!($($arg)*)).unwrap();
    });
}

#[doc(hidden)]
pub fn print_args_unsafe(args: fmt::Arguments) -> fmt::Result {
    unsafe { WRITER.force_unlock(); }
    print_args(args)
}

/// This is UNSAFE because it bypasses the VGA Buffer lock. Use println!() instead. 
/// Should only be used in exception contexts and early bring-up code
/// before the console-based println!() macro is available. 
#[macro_export]
macro_rules! println_unsafe {
    ($fmt:expr) => (print_unsafe!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => (print_unsafe!(concat!($fmt, "\n"), $($arg)*));
}



pub fn print_string(s: String) -> fmt::Result {
    print_str(s.as_str())
}

pub fn print_str(s: &str) -> fmt::Result {
    use core::fmt::Write;
    WRITER.lock().write_str(s)
}

pub fn print_args(args: fmt::Arguments) -> fmt::Result {
    use core::fmt::Write;
    WRITER.lock().write_fmt(args)
}

pub fn clear_screen() {
    let mut locked_writer = WRITER.lock(); 
    for _ in 0..BUFFER_HEIGHT {
        locked_writer.new_line();
    }
}

pub fn show_splash_screen() {
    print_str(WELCOME_STRING);
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

pub struct Writer {
    column_position: usize,
    color_code: ColorCode,
    buffer: Unique<Buffer>,
}

impl Writer {
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            byte => {
                if self.column_position >= BUFFER_WIDTH {
                    self.new_line();
                }
                let row = BUFFER_HEIGHT - 1;
                let col = self.column_position;

                let color_code = self.color_code;

                self.buffer().chars[row][col].write(ScreenChar {
                                                        ascii_character: byte,
                                                        color_code: color_code,
                                                    });
                self.column_position += 1;
            }
        }
    }

    fn buffer(&mut self) -> &mut Buffer {
        unsafe { self.buffer.as_mut() }
    }

    fn new_line(&mut self) {
        for row in 1..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                let buffer = self.buffer();
                let character = buffer.chars[row][col].read();
                buffer.chars[row - 1][col].write(character);
            }
        }
        self.clear_row(BUFFER_HEIGHT - 1);
        self.column_position = 0;
    }

    fn clear_row(&mut self, row: usize) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        for col in 0..BUFFER_WIDTH {
            self.buffer().chars[row][col].write(blank);
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> ::core::fmt::Result {
        
        ::drivers::serial_port::serial_out(s); // mirror to serial port
        
        for byte in s.bytes() {
            self.write_byte(byte)
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct ColorCode(u8);

impl ColorCode {
    const fn new(foreground: Color, background: Color) -> ColorCode {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}

struct Buffer {
    chars: [[Volatile<ScreenChar>; BUFFER_WIDTH]; BUFFER_HEIGHT],
}


// this doesn't line up as shown here because of the escaped backslashes,
// but it lines up properly when printed :)
static WELCOME_STRING: &'static str = "\n
    ____           _    __       _        ___  ____  
   |  _ \\ ___  ___| |_ / _|_   _| |      / _ \\/ ___| 
   | |_) / _ \\/ __| __| |_| | | | |     | | | \\___ \\ 
   |  _ <  __/\\__ \\ |_|  _| |_| | |     | |_| |___) |
   |_| \\_\\___||___/\\__|_|  \\__,_|_|      \\___/|____/ \n";
                                               