//! Support for basic printing to a simple 80x25 text-mode VGA buffer. 
//!
//! Does not support scrolling, cursors, or any other features of a regular drawn framebuffer.

#![no_std]
#![feature(ptr_internals)]

extern crate kernel_config;
extern crate logger;
extern crate spin;
extern crate volatile;


use core::fmt;
use core::ptr::Unique;
use spin::Mutex;
use volatile::Volatile;


/// The VBE/VESA standard defines the text mode VGA buffer to start at this address.
/// We must rely on the early bootstrap code to identity map this address.
const VGA_BUFFER_VIRTUAL_ADDR: usize = 0xb8000;

/// height of the VGA text window
const BUFFER_HEIGHT: usize = 25;
/// width of the VGA text window
const BUFFER_WIDTH: usize = 80;


/// The singleton VGA writer instance that writes to the VGA text buffer.
static EARLY_VGA_WRITER: Mutex<VgaBuffer> = Mutex::new(
    VgaBuffer {
        column_position: 0,
        // SAFE: the assembly boot up code ensures this is mapped into memory.
        buffer: unsafe { Unique::new_unchecked((VGA_BUFFER_VIRTUAL_ADDR) as *mut _) },
    }
);


// Note: we can't put this cfg block inside the macro, because then it will be
//       enabled based on the chosen features of the foreign crate that
//       *calls* this macro, rather than the features activated in *this* crate.
#[cfg(feature = "bios")]
#[macro_export]
macro_rules! print_raw {
    ($($arg:tt)*) => ({
        let _ = $crate::print_args_raw(format_args!($($arg)*));
    });
}

// Note: we can't put this cfg block inside the macro, because then it will be
//       enabled based on the chosen features of the foreign crate that
//       *calls* this macro, rather than the features activated in *this* crate.
#[cfg(not(feature = "bios"))]
#[macro_export]
macro_rules! print_raw {
    ($($arg:tt)*) => ({
        // to silence warnings about unused variables
        let _ = format_args!($($arg)*);
    });
}

#[macro_export]
macro_rules! println_raw {
    ($fmt:expr) => ($crate::print_raw!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::print_raw!(concat!($fmt, "\n"), $($arg)*));
}

#[doc(hidden)]
pub fn print_args_raw(args: fmt::Arguments) -> fmt::Result {
    use core::fmt::Write;
    EARLY_VGA_WRITER.lock().write_fmt(args)
}


type VgaTextBufferLine = [Volatile<ScreenChar>; BUFFER_WIDTH];
type VgaTextBuffer     = [VgaTextBufferLine; BUFFER_HEIGHT];


struct VgaBuffer {
    column_position: usize,
    buffer: Unique<VgaTextBuffer>,
}
impl VgaBuffer {
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            byte => {
                if self.column_position >= BUFFER_WIDTH {
                    self.new_line();
                }
                let row = BUFFER_HEIGHT - 1;
                let col = self.column_position;

                self.buffer()[row][col].write(
                    ScreenChar {
                        ascii_character: byte,
                        color_code: ColorCode::default(),
                    }
                );
                self.column_position += 1;
            }
        }
    }

    fn new_line(&mut self) {
        for row in 1..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                let character = self.buffer()[row][col].read();
                self.buffer()[row - 1][col].write(character);
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
            self.buffer()[row][col].write(blank);
        }
    }

    fn buffer(&mut self) -> &mut [VgaTextBufferLine] {
        // SAFE: mutability is protected by the lock surrounding the RawVgaBuffer instance
        unsafe { self.buffer.as_mut() }
    }
}
impl fmt::Write for VgaBuffer {
    fn write_str(&mut self, s: &str) -> ::core::fmt::Result {
        
        // mirror DIRECTLY to serial port, do not use the log statements
        // because that can introduce an infinite loop when mirror_to_serial is enabled.
        let ret = logger::write_str(s); 
        
        for byte in s.bytes() {
            self.write_byte(byte)
        }
        ret
    }
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

#[derive(Debug, Clone, Copy)]
pub struct ColorCode(u8);

impl ColorCode {
    pub const fn new(foreground: Color, background: Color) -> ColorCode {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }
}

impl Default for ColorCode {
    fn default() -> ColorCode {
        ColorCode::new(Color::LightGreen, Color::Black)
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}
impl ScreenChar {
    pub const fn new(ascii: u8, color: ColorCode) -> ScreenChar {
        ScreenChar {
            ascii_character: ascii,
            color_code: color,
        }
    }
}
