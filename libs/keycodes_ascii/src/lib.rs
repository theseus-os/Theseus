#![allow(dead_code)]
#![feature(const_fn)]
#![feature(try_from)]

// #![feature(plugin)]
// #![plugin(phf_macros)]
// extern crate phf;

#[macro_use] extern crate enum_primitive;
extern crate num;

use std::error::Error;
use std::convert::TryFrom;
use std::fmt;
use enum_primitive::FromPrimitive;


// TODO: use these tables and tips:
// https://sourceforge.net/p/oszur11/code/ci/master/tree/Chapter_06_Shell/04_Makepp/arch/i386/arch/devices/i8042.c



#[derive(Debug, Copy, Clone)]
pub struct KeyboardModifiers {
    control: bool,
    alt: bool, 
    shift: bool,
    caps_lock: bool,
    num_lock: bool,
}

impl KeyboardModifiers {
    pub fn new() -> KeyboardModifiers {
        KeyboardModifiers {
            control: false,
            alt: false, 
            shift: false,
            caps_lock: false,
            num_lock: false,
        }
    }
}


pub static KEY_RELEASED_OFFSET: u8 = 128;


pub const fn get_keycode(scan_code: u8) -> Option<Keycode> {
    FromPrimitive::from_u8(scan_code)
}


/// obtains the ascii value for a raw scancode under the given modifiers
pub const fn scan_to_ascii(modifiers: &KeyboardModifiers, scan_code: u8) -> Option<u8> {
    if let Some(keycode) = get_keycode(scan_code) {
        key_to_ascii(modifiers, keycode)
    }
    else {
        None
    }
}

/// obtains the ascii value for a keycode under the given modifiers
pub const fn key_to_ascii(modifiers: &KeyboardModifiers, keycode: Keycode) -> Option<u8> {
    // handle shift key being pressed
    if modifiers.shift {
        // if shift is pressed and caps lock is on, give a regular lowercase letter
        if modifiers.caps_lock && is_letter(keycode) {
            return to_ascii(keycode);
        }
        // if shift is pressed and caps lock is not, give a regular shifted key
        else {
            return to_ascii_shifted(keycode)
        }
    }
    
    // just a regular caps_lock, no shift pressed 
    // (we already covered the shift && caps_lock scenario above)
    if modifiers.caps_lock {
        if is_letter(keycode) {
            return to_ascii_shifted(keycode)
        }
        else {
            return to_ascii(keycode)
        }
    }

    None
    
    // TODO: handle numlock
}


// impl PhfHash for Keycode {
//     fn phf_hash<H: Hasher>(&self, state: &mut H) {
//         match *self {
//             x => (x as u8).phf_hash(state),
//         }
//     }
// }


// impl Hash for Key {
//     fn hash<S: Hasher>(&self, state: &mut S) {
//         match *self {
//             x => (x as u8).phf_hash(state), 
//         }
//     }
// }




enum_from_primitive! {
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Keycode {
    OverflowError = 0,
    Escape,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    Num0,
    Minus,
    Equals,
    Backspace,
    Tab,
    Q,
    W,
    E,
    R,
    T,
    Y,
    U,
    I,
    O,
    P,
    LeftBracket,
    RightBracket,
    Enter,
    Control,
    A,
    S,
    D,
    F,
    G,
    H,
    J,
    K,
    L,
    Semicolon,
    Quote,
    Backtick,
    LeftShift,
    Backslash,
    Z,
    X,
    C,
    V,
    B,
    N,
    M,
    Comma,
    Period,
    Slash,
    RightShift,
    PadMultiply, // Also PrintScreen
    Alt,
    Space,
    CapsLock,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    NumLock,
    ScrollLock,
    Home, // Also Pad7
    Up, // Also Pad8
    PageUp, // Also Pad9
    PadMinus,
    Left, // Also Pad4
    Pad5,
    Right, // Also Pad6
    PadPlus,
    End, // Also Pad1
    Down, // Also Pad2
    PageDown, // Also Pad3
    Insert, // Also Pad0
    Delete, // Also PadDecimal
    Unknown1,
    Unknown2,
    NonUsBackslash,
    F11,
    F12,
    Pause,
    Unknown3,
    LeftGui,
    RightGui,
    Menu,
}
} // end of enum_from_primitive!


#[derive(Debug)]
pub struct TryFromKeycodeError { 
    scan_code: u8,
}

impl Error for TryFromKeycodeError {
    fn description(&self) -> &str {
        "out of range integral type conversion attempted"
    }
}

impl fmt::Display for TryFromKeycodeError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.write_str(self.description())
    }
}


impl TryFrom<u8> for Keycode {
    type Error = TryFromKeycodeError;
    fn try_from(original: u8) -> Result<Keycode, TryFromKeycodeError> {
        let kc = get_keycode(original);
        match kc {
            Some(x) => Ok(x),
            fail => Err(TryFromKeycodeError{ scan_code: original }),
        }
    }
}



/// maps a Keycode to ASCII values (u8) without any "shift" modifiers.
const fn to_ascii(keycode: Keycode) -> Option<u8> {
    match keycode {
        Keycode::Escape => Some(27 as u8),
        Keycode::Num1 => Some('1' as u8),
        Keycode::Num2 => Some('2' as u8),
        Keycode::Num3 => Some('3' as u8),
        Keycode::Num4 => Some('4' as u8),
        Keycode::Num5 => Some('5' as u8),
        Keycode::Num6 => Some('6' as u8),
        Keycode::Num7 => Some('7' as u8),
        Keycode::Num8 => Some('8' as u8),
        Keycode::Num9 => Some('9' as u8),
        Keycode::Num0 => Some('0' as u8), 
        Keycode::Minus => Some('-' as u8),
        Keycode::Equals => Some('=' as u8),
        Keycode::Backspace => Some(8 as u8), 
        Keycode::Tab => Some(9 as u8),
        Keycode::Q => Some('q' as u8),
        Keycode::W => Some('w' as u8),
        Keycode::E => Some('e' as u8),
        Keycode::R => Some('r' as u8),
        Keycode::T => Some('t' as u8),
        Keycode::Y => Some('y' as u8),
        Keycode::U => Some('u' as u8), 
        Keycode::I => Some('i' as u8), 
        Keycode::O => Some('o' as u8),
        Keycode::P => Some('p' as u8),
        Keycode::LeftBracket => Some('[' as u8),
        Keycode::RightBracket => Some(']' as u8),
        Keycode::Enter => Some(13 as u8), 
        Keycode::A => Some('a' as u8),
        Keycode::S => Some('s' as u8),
        Keycode::D => Some('d' as u8),
        Keycode::F => Some('f' as u8),
        Keycode::G => Some('g' as u8),
        Keycode::H => Some('h' as u8),
        Keycode::J => Some('j' as u8),
        Keycode::K => Some('k' as u8),
        Keycode::L => Some('l' as u8),
        Keycode::Semicolon => Some(';' as u8),
        Keycode::Quote => Some('\'' as u8), 
        Keycode::Backtick => Some('`' as u8),
        Keycode::Backslash => Some('\\' as u8),
        Keycode::Z => Some('z' as u8),
        Keycode::X => Some('x' as u8),
        Keycode::C => Some('c' as u8),
        Keycode::V => Some('v' as u8),
        Keycode::B => Some('b' as u8),
        Keycode::N => Some('n' as u8),
        Keycode::M => Some('m' as u8),
        Keycode::Comma => Some(',' as u8),
        Keycode::Period => Some('.' as u8),
        Keycode::Slash => Some('/' as u8),
        Keycode::Space => Some(' ' as u8),
        Keycode::PadMultiply => Some('*' as u8),
        Keycode::PadMinus => Some('-' as u8),
        Keycode::PadPlus => Some('+' as u8),
        // PadSlash / PadDivide?? 

        _ => None,
    }
}


/// same as to_ascii, but adds the effect of the "shift" modifier key. 
/// If a Keycode's ascii value doesn't change when shifted,
/// then it defaults to it's non-shifted value as returned by to_ascii().
const fn to_ascii_shifted(keycode: Keycode) -> Option<u8> {
    match keycode {
        Keycode::Num1 => Some('!' as u8),
        Keycode::Num2 => Some('@' as u8),
        Keycode::Num3 => Some('#' as u8),
        Keycode::Num4 => Some('$' as u8),
        Keycode::Num5 => Some('%' as u8),
        Keycode::Num6 => Some('^' as u8),
        Keycode::Num7 => Some('&' as u8),
        Keycode::Num8 => Some('*' as u8),
        Keycode::Num9 => Some('(' as u8),
        Keycode::Num0 => Some(')' as u8), 
        Keycode::Minus => Some('_' as u8),
        Keycode::Equals => Some('+' as u8),
        Keycode::Q => Some('Q' as u8),
        Keycode::W => Some('W' as u8),
        Keycode::E => Some('E' as u8),
        Keycode::R => Some('R' as u8),
        Keycode::T => Some('T' as u8),
        Keycode::Y => Some('Y' as u8),
        Keycode::U => Some('U' as u8), 
        Keycode::I => Some('I' as u8), 
        Keycode::O => Some('O' as u8),
        Keycode::P => Some('P' as u8),
        Keycode::LeftBracket => Some('{' as u8),
        Keycode::RightBracket => Some('}' as u8),
        Keycode::A => Some('A' as u8),
        Keycode::S => Some('S' as u8),
        Keycode::D => Some('D' as u8),
        Keycode::F => Some('F' as u8),
        Keycode::G => Some('G' as u8),
        Keycode::H => Some('H' as u8),
        Keycode::J => Some('J' as u8),
        Keycode::K => Some('K' as u8),
        Keycode::L => Some('L' as u8),
        Keycode::Semicolon => Some(':' as u8),
        Keycode::Quote => Some('"' as u8), 
        Keycode::Backtick => Some('~' as u8),
        Keycode::Backslash => Some('|' as u8),
        Keycode::Z => Some('Z' as u8),
        Keycode::X => Some('X' as u8),
        Keycode::C => Some('C' as u8),
        Keycode::V => Some('V' as u8),
        Keycode::B => Some('B' as u8),
        Keycode::N => Some('N' as u8),
        Keycode::M => Some('M' as u8),
        Keycode::Comma => Some('<' as u8),
        Keycode::Period => Some('>' as u8),
        Keycode::Slash => Some('?' as u8),
        
        other => to_ascii(other),
    }
}


/// returns true if the given keycode was a letter from A-Z
pub const fn is_letter(keycode: Keycode) -> bool {
    match keycode {
        Keycode::Q |
        Keycode::W |
        Keycode::E |
        Keycode::R |
        Keycode::T |
        Keycode::Y |
        Keycode::U |
        Keycode::I |
        Keycode::O |
        Keycode::P |
        Keycode::A |
        Keycode::S |
        Keycode::D |
        Keycode::F |
        Keycode::G |
        Keycode::H |
        Keycode::J |
        Keycode::K |
        Keycode::L |
        Keycode::Z |
        Keycode::X |
        Keycode::C |
        Keycode::V |
        Keycode::B |
        Keycode::N |
        Keycode::M  => true,

        _ => false,
    }
}
