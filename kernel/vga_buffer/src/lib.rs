//! The vga buffer that implements basic printing in VGA text mode.

#![no_std]
#![feature(alloc)]
#![feature(const_fn)]
#![feature(unique)]
#![feature(ptr_internals)]

// #[macro_use] extern crate log;
extern crate alloc;
extern crate kernel_config;
extern crate port_io;
extern crate serial_port;
extern crate spin;
extern crate volatile;
extern crate text_display;

#[macro_use] extern crate log;

use text_display::TextDisplay;
use core::fmt;
use core::ptr::Unique;
use alloc::Vec;
use core::mem;
use kernel_config::memory::KERNEL_OFFSET;
use port_io::Port;
use spin::Mutex;
use volatile::Volatile;

/// defined by x86's physical memory maps
const VGA_BUFFER_VIRTUAL_ADDR: usize = 0xb8000 + KERNEL_OFFSET;

/// height of the VGA text window
const BUFFER_HEIGHT: usize = 25;
/// width of the VGA text window
const BUFFER_WIDTH: usize = 80;

pub mod raw;

type Line = [ScreenChar; BUFFER_WIDTH];

const BLANK_LINE: Line = [ScreenChar::new(b' ', ColorCode::new(Color::LightGreen, Color::Black)); BUFFER_WIDTH];
static CURSOR_PORT_START: Mutex<Port<u8>> = Mutex::new(Port::new(0x3D4));
static CURSOR_PORT_END: Mutex<Port<u8>> = Mutex::new(Port::new(0x3D5));
static AUXILLARY_ADDR: Mutex<Port<u8>> = Mutex::new(Port::new(0x3E0));

const UNLOCK_SEQ_1: u8 = 0x0A;
const UNLOCK_SEQ_2: u8 = 0x0B;
const UNLOCK_SEQ_3: u8 = 0xC0;
const UNLOCK_SEQ_4: u8 = 0xE0;
const UPDATE_SEQ_1: u8 = 0x0E;
const UPDATE_SEQ_2: u8 = 0x0F;
const UPDATE_SEQ_3: u16 = 0xFF;
const CURSOR_START: u8 = 0b00000001;
const CURSOR_END: u8 = 0b00010000;
const RIGHT_BIT_SHIFT: u8 = 8;
const DISABLE_SEQ_1: u8 = 0x0A;
const DISABLE_SEQ_2: u8 = 0x20;


/// An instance of a VGA text buffer which can be displayed to the screen.
pub struct VgaBuffer { }
impl VgaBuffer {
    pub fn new() -> VgaBuffer {
        VgaBuffer { }
    }
}


/// Implements TextDisplay trait for vga buffer.
/// set_cursor() should accept coordinates within those specified by get_dimensions() and display to window
impl TextDisplay for VgaBuffer {
    /// Update the cursor based on the given x and y coordinates (sourced from OsDev Wiki),
    /// which correspond to the column and row (line) respectively 
    fn set_cursor(&self, x: u16, y: u16) {
        let pos: u16 = y * BUFFER_WIDTH as u16 + x;

        unsafe {
            // enables cursor 
            CURSOR_PORT_START.lock().write(UNLOCK_SEQ_1);
            let temp_read: u8 = (CURSOR_PORT_END.lock().read() & UNLOCK_SEQ_3) | CURSOR_START;
            CURSOR_PORT_END.lock().write(temp_read);
            CURSOR_PORT_START.lock().write(UNLOCK_SEQ_2);
            let temp_read2 = (AUXILLARY_ADDR.lock().read() & UNLOCK_SEQ_4) | CURSOR_END;
            CURSOR_PORT_END.lock().write(temp_read2);
            // updates cursor 
            CURSOR_PORT_START.lock().write(UPDATE_SEQ_2);
            CURSOR_PORT_END.lock().write((pos & UPDATE_SEQ_3) as u8);
            CURSOR_PORT_START.lock().write(UPDATE_SEQ_1);
            CURSOR_PORT_END.lock().write(((pos >> RIGHT_BIT_SHIFT) & UPDATE_SEQ_3) as u8);
        }
        return;
    }

    /// Disables the cursor
    /// Still maintains the cursor's position
    fn disable_cursor(&self) {
        unsafe {
            CURSOR_PORT_START.lock().write(DISABLE_SEQ_1);
            CURSOR_PORT_END.lock().write(DISABLE_SEQ_2);
        }
    }

    /// Returns a tuple containing (buffer height, buffer width)
    fn get_dimensions(&self) -> (usize, usize) {
        (BUFFER_WIDTH, BUFFER_HEIGHT)
    }

    /// Requires that a str slice that will exactly fit the vga buffer
    /// The calculation is done inside the console crate by the print_to_vga function and associated methods
    /// Parses the string into line objects and then prints them onto the vga buffer
    fn display_string(&mut self, slice: &str) -> Result<(), &'static str> {
        let mut curr_column = 0;
        let mut new_line = BLANK_LINE;
        let mut cursor_pos = 0;
        let mut i = 0;
        use core::ptr::write_volatile;
        // iterates through the string slice and puts it into lines that will fit on the vga buffer
        for byte in slice.bytes() {
            if byte == b'\n' { // if we reach a line break
                let addr = (VGA_BUFFER_VIRTUAL_ADDR + i * mem::size_of::<Line>()) as *mut Line;
                unsafe { write_volatile(addr, new_line); }
                new_line = BLANK_LINE;
                cursor_pos += BUFFER_WIDTH - curr_column;
                curr_column = 0;
                i += 1;
            } else { // if we reach the end of the line with no line break
                if curr_column == BUFFER_WIDTH {
                    curr_column = 0;
                    let addr = (VGA_BUFFER_VIRTUAL_ADDR + i * mem::size_of::<Line>()) as *mut Line;
                    unsafe { write_volatile(addr, new_line); }
                    i += 1;
                    new_line = BLANK_LINE;
                }
                new_line[curr_column] = ScreenChar::new(byte, ColorCode::default());
                curr_column += 1;
                cursor_pos += 1;
            }
        }

        // writes the last line not covered in the loop
        let addr = (VGA_BUFFER_VIRTUAL_ADDR + i * mem::size_of::<Line>()) as *mut Line;
        unsafe { write_volatile(addr, new_line); }

        // fills the remainder of the vga buffer with blank lines if there any unfilled ones
        if i < BUFFER_HEIGHT -1 {
            for j in i+1..BUFFER_HEIGHT {
                let addr = (VGA_BUFFER_VIRTUAL_ADDR + j * mem::size_of::<Line>()) as *mut Line;
                // trace!("   writing BLANK ({}) at addr {:#X}", i, addr as usize);
                unsafe { write_volatile(addr, BLANK_LINE); }
            }
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
