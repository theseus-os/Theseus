//! Support for basic printing to a simple 80x25 text-mode VGA display. 
//!
//! Does not support user scrolling, cursors, or any other advanced features.

#![no_std]
#![feature(ptr_internals)]
#![feature(const_option)]

use core::{fmt::{self, Write}, ptr::Unique};
use volatile::Volatile;

/// The VBE/VESA standard defines the text mode VGA buffer to start at this address.
/// We must rely on the early bootstrap code to identity map this address.
#[cfg(target_arch = "x86_64")] // ensures build failure on non-x86 platforms
const VGA_BUFFER_VIRTUAL_ADDR: usize = 0xb8000;

/// height of the VGA text window
const BUFFER_HEIGHT: usize = 25;
/// width of the VGA text window
const BUFFER_WIDTH: usize = 80;


type VgaTextBufferLine = [Volatile<ScreenChar>; BUFFER_WIDTH];
type VgaTextBuffer     = [VgaTextBufferLine; BUFFER_HEIGHT];


pub struct VgaBuffer {
    column_position: usize,
    buffer: Unique<VgaTextBuffer>,
}
impl VgaBuffer {
    pub const fn new() -> Self {
        VgaBuffer {
            column_position: 0,
            buffer: Unique::new((VGA_BUFFER_VIRTUAL_ADDR) as *mut _).unwrap(),
        }
    }

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
        // SAFETY: this function requires a `&mut` reference, ensuring exclusivity.
        unsafe { self.buffer.as_mut() }
    }
}

impl Write for VgaBuffer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.write_byte(byte)
        }
        Ok(())
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
