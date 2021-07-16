#![allow(dead_code)]
#![no_std]


#[macro_use] extern crate bitflags;

// use core::cell::RefCell;

// TODO: use these tables and tips:
// https://sourceforge.net/p/oszur11/code/ci/master/tree/Chapter_06_Shell/04_Makepp/arch/i386/arch/devices/i8042.c

// TODO: seems like we actually can use phf crates
// we can use the "core" feature enables libcore instead of libstd
// you can use number literals like so: 
/*
static MYMAP: phf::Map<u8, &'static Keycode> = phf_map! {
    0u8 => Keycode::BLAH,
    1u8 => Keycode::BLAH2,
    ... etc ...
}
*/

// the implementation here follows the rule of representation, 
// which is to use complicated data structures to permit simpler logic. 


bitflags! {
    /// The set of modifier keys that can be held down while other keys are pressed.
    /// 
    /// To save space, this is expressed using bitflags 
    /// rather than a series of individual booleans, 
    /// because Rust's `bool` type is a whole byte.
    pub struct KeyboardModifiers: u16 {
        const CONTROL_LEFT    = 1 <<  0;
        const CONTROL_RIGHT   = 1 <<  1;
        const SHIFT_LEFT      = 1 <<  2;
        const SHIFT_RIGHT     = 1 <<  3;
        const ALT             = 1 <<  4;
        const ALT_GR          = 1 <<  5;
        const SUPER_KEY_LEFT  = 1 <<  6;
        const SUPER_KEY_RIGHT = 1 <<  7;
        const CAPS_LOCK       = 1 <<  8;
        const NUM_LOCK        = 1 <<  9;
        const SCROLL_LOCK     = 1 << 10;
    }
}

impl Default for KeyboardModifiers {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardModifiers {
    /// Returns a new `KeyboardModifiers` struct with no keys pressed.
    pub const fn new() -> KeyboardModifiers {
        Self::empty()
    }

    /// Returns `true` if a `Shift` key is held down (either left or right).
    #[inline(always)]
    pub fn is_shift(&self) -> bool {
        self.intersects(Self::SHIFT_LEFT | Self::SHIFT_RIGHT)
    }

    /// Returns `true` if a `Control` key is held down (either left or right).
    #[inline(always)]
    pub fn is_control(&self) -> bool {
        self.intersects(Self::CONTROL_LEFT | Self::CONTROL_RIGHT)
    }

    /// Returns `true` if the `Alt` key is held down.
    #[inline(always)]
    pub fn is_alt(&self) -> bool {
        self.intersects(Self::ALT)
    }

    /// Returns `true` if the `AltGr` key is held down.
    #[inline(always)]
    pub fn is_alt_gr(&self) -> bool {
        self.intersects(Self::ALT_GR)
    }

    /// Returns `true` if a Super key is held down (either left or right).
    /// 
    /// Examples include the Windows key, the Meta key, the command key, etc.
    #[inline(always)]
    pub fn is_super_key(&self) -> bool {
        self.intersects(Self::SUPER_KEY_LEFT | Self::SUPER_KEY_RIGHT)
    }

    /// Returns `true` if the `Caps Lock` key is held down.
    #[inline(always)]
    pub fn is_caps_lock(&self) -> bool {
        self.intersects(Self::CAPS_LOCK)
    }

    /// Returns `true` if the `Num Lock` key is held down.
    #[inline(always)]
    pub fn is_num_lock(&self) -> bool {
        self.intersects(Self::NUM_LOCK)
    }

    /// Returns `true` if the `Scroll Lock` key is held down.
    #[inline(always)]
    pub fn is_scroll_lock(&self) -> bool {
        self.intersects(Self::SCROLL_LOCK)
    }
}

/// Whether a keyboard event was a key press or a key released.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum KeyAction {
    Pressed,
    Released,
}

/// The KeyEvent that should be delivered to applications upon a keyboard action.
#[derive(Debug, Copy, Clone)]
pub struct KeyEvent {
    pub keycode: Keycode,
    pub action: KeyAction,
    pub modifiers: KeyboardModifiers,
}

impl KeyEvent {
    pub fn new(keycode: Keycode, action: KeyAction, modifiers: KeyboardModifiers,) -> KeyEvent {
        KeyEvent {
            keycode, 
            action,
            modifiers,
        }
    }
}

/// The offset that a keyboard adds to the scancode
/// to indicate that the key was released rather than pressed. 
/// So if a scancode of `1` means a key `foo` was pressed,
/// a scancode of `129` (1 + 128) means that key `foo` was released. 
pub const KEY_RELEASED_OFFSET: u8 = 128;

/// convenience function for obtaining the ascii value for a raw scancode under the given modifiers
pub fn scancode_to_ascii(modifiers: KeyboardModifiers, scan_code: u8) -> Option<char> {
	Keycode::from_scancode(scan_code).and_then(|k| k.to_ascii(modifiers))
}


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
    SuperKeyLeft,
    SuperKeyRight,
    Menu,
} 




impl Keycode {

    pub fn from_scancode(scancode: u8)  -> Option<Keycode> {
        match scancode {
            0 => Some(Keycode::OverflowError),
            1 => Some(Keycode::Escape),
            2 => Some(Keycode::Num1),
            3 => Some(Keycode::Num2),
            4 => Some(Keycode::Num3),
            5 => Some(Keycode::Num4),
            6 => Some(Keycode::Num5),
            7 => Some(Keycode::Num6),
            8 => Some(Keycode::Num7),
            9 => Some(Keycode::Num8),
            10 => Some(Keycode::Num9),
            11 => Some(Keycode::Num0),
            12 => Some(Keycode::Minus),
            13 => Some(Keycode::Equals),
            14 => Some(Keycode::Backspace),
            15 => Some(Keycode::Tab),
            16 => Some(Keycode::Q),
            17 => Some(Keycode::W),
            18 => Some(Keycode::E),
            19 => Some(Keycode::R),
            20 => Some(Keycode::T),
            21 => Some(Keycode::Y),
            22 => Some(Keycode::U),
            23 => Some(Keycode::I),
            24 => Some(Keycode::O),
            25 => Some(Keycode::P),
            26 => Some(Keycode::LeftBracket),
            27 => Some(Keycode::RightBracket),
            28 => Some(Keycode::Enter),
            29 => Some(Keycode::Control),
            30 => Some(Keycode::A),
            31 => Some(Keycode::S),
            32 => Some(Keycode::D),
            33 => Some(Keycode::F),
            34 => Some(Keycode::G),
            35 => Some(Keycode::H),
            36 => Some(Keycode::J),
            37 => Some(Keycode::K),
            38 => Some(Keycode::L),
            39 => Some(Keycode::Semicolon),
            40 => Some(Keycode::Quote),
            41 => Some(Keycode::Backtick),
            42 => Some(Keycode::LeftShift),
            43 => Some(Keycode::Backslash),
            44 => Some(Keycode::Z),
            45 => Some(Keycode::X),
            46 => Some(Keycode::C),
            47 => Some(Keycode::V),
            48 => Some(Keycode::B),
            49 => Some(Keycode::N),
            50 => Some(Keycode::M),
            51 => Some(Keycode::Comma),
            52 => Some(Keycode::Period),
            53 => Some(Keycode::Slash),
            54 => Some(Keycode::RightShift),
            55 => Some(Keycode::PadMultiply), // Also PrintScreen
            56 => Some(Keycode::Alt),
            57 => Some(Keycode::Space),
            58 => Some(Keycode::CapsLock),
            59 => Some(Keycode::F1),
            60 => Some(Keycode::F2),
            61 => Some(Keycode::F3),
            62 => Some(Keycode::F4),
            63 => Some(Keycode::F5),
            64 => Some(Keycode::F6),
            65 => Some(Keycode::F7),
            66 => Some(Keycode::F8),
            67 => Some(Keycode::F9),
            68 => Some(Keycode::F10),
            69 => Some(Keycode::NumLock),
            70 => Some(Keycode::ScrollLock),
            71 => Some(Keycode::Home), // Also Pad7
            72 => Some(Keycode::Up), // Also Pad8
            73 => Some(Keycode::PageUp), // Also Pad9
            74 => Some(Keycode::PadMinus),
            75 => Some(Keycode::Left), // Also Pad4
            76 => Some(Keycode::Pad5),
            77 => Some(Keycode::Right), // Also Pad6
            78 => Some(Keycode::PadPlus),
            79 => Some(Keycode::End), // Also Pad1
            80 => Some(Keycode::Down), // Also Pad2
            81 => Some(Keycode::PageDown), // Also Pad3
            82 => Some(Keycode::Insert), // Also Pad0
            83 => Some(Keycode::Delete), // Also PadDecimal
            84 => Some(Keycode::Unknown1),
            85 => Some(Keycode::Unknown2),
            86 => Some(Keycode::NonUsBackslash),
            87 => Some(Keycode::F11),
            88 => Some(Keycode::F12),
            89 => Some(Keycode::Pause),
            90 => Some(Keycode::Unknown3),
            91 => Some(Keycode::SuperKeyLeft),
            92 => Some(Keycode::SuperKeyRight),
            93 => Some(Keycode::Menu),

            _ => None,
        }
    }



    /// Obtains the ascii value for a keycode under the given modifiers
    pub fn to_ascii(&self, modifiers: KeyboardModifiers) -> Option<char> {
        // handle shift key being pressed
        if modifiers.is_shift() {
            // if shift is pressed and caps lock is on, give a regular lowercase letter
            if modifiers.is_caps_lock() && self.is_letter() {
                return self.as_ascii();
            }
            // if shift is pressed and caps lock is not, give a regular shifted key
            else {
                return self.as_ascii_shifted()
            }
        }
        
        // just a regular caps_lock, no shift pressed 
        // (we already covered the shift && caps_lock scenario above)
        if modifiers.is_caps_lock() {
            if self.is_letter() {
                return self.as_ascii_shifted()
            }
            else {
                return self.as_ascii()
            }
        }

        // default to regular ascii value
        self.as_ascii()
        
        // TODO: handle numlock
    }



    /// returns true if this keycode was a letter from A-Z
    pub fn is_letter(&self) -> bool {
        match *self {
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



    /// maps a Keycode to ASCII char values without any "shift" modifiers.
    fn as_ascii(&self) -> Option<char> {
        match *self {
            Keycode::Escape => Some(char::from(27)),
            Keycode::Num1 => Some('1'),
            Keycode::Num2 => Some('2'),
            Keycode::Num3 => Some('3'),
            Keycode::Num4 => Some('4'),
            Keycode::Num5 => Some('5'),
            Keycode::Num6 => Some('6'),
            Keycode::Num7 => Some('7'),
            Keycode::Num8 => Some('8'),
            Keycode::Num9 => Some('9'),
            Keycode::Num0 => Some('0'), 
            Keycode::Minus => Some('-'),
            Keycode::Equals => Some('='),
            Keycode::Backspace => Some(char::from(8)), 
            Keycode::Tab => Some(char::from(9)),
            Keycode::Q => Some('q'),
            Keycode::W => Some('w'),
            Keycode::E => Some('e'),
            Keycode::R => Some('r'),
            Keycode::T => Some('t'),
            Keycode::Y => Some('y'),
            Keycode::U => Some('u'), 
            Keycode::I => Some('i'), 
            Keycode::O => Some('o'),
            Keycode::P => Some('p'),
            Keycode::LeftBracket => Some('['),
            Keycode::RightBracket => Some(']'),
            Keycode::Enter => Some('\n'), 
            Keycode::A => Some('a'),
            Keycode::S => Some('s'),
            Keycode::D => Some('d'),
            Keycode::F => Some('f'),
            Keycode::G => Some('g'),
            Keycode::H => Some('h'),
            Keycode::J => Some('j'),
            Keycode::K => Some('k'),
            Keycode::L => Some('l'),
            Keycode::Semicolon => Some(';'),
            Keycode::Quote => Some('\''), 
            Keycode::Backtick => Some('`'),
            Keycode::Backslash => Some('\\'),
            Keycode::Z => Some('z'),
            Keycode::X => Some('x'),
            Keycode::C => Some('c'),
            Keycode::V => Some('v'),
            Keycode::B => Some('b'),
            Keycode::N => Some('n'),
            Keycode::M => Some('m'),
            Keycode::Comma => Some(','),
            Keycode::Period => Some('.'),
            Keycode::Slash => Some('/'),
            Keycode::Space => Some(' '),
            Keycode::PadMultiply => Some('*'),
            Keycode::PadMinus => Some('-'),
            Keycode::PadPlus => Some('+'),
            // PadSlash / PadDivide?? 

            _ => None,
        }
    }


    /// same as as_ascii, but adds the effect of the "shift" modifier key. 
    /// If a Keycode's ascii value doesn't change when shifted,
    /// then it defaults to it's non-shifted value as returned by as_ascii().
    fn as_ascii_shifted(&self) -> Option<char> {
        match *self {
            Keycode::Num1 => Some('!'),
            Keycode::Num2 => Some('@'),
            Keycode::Num3 => Some('#'),
            Keycode::Num4 => Some('$'),
            Keycode::Num5 => Some('%'),
            Keycode::Num6 => Some('^'),
            Keycode::Num7 => Some('&'),
            Keycode::Num8 => Some('*'),
            Keycode::Num9 => Some('('),
            Keycode::Num0 => Some(')'), 
            Keycode::Minus => Some('_'),
            Keycode::Equals => Some('+'),
            Keycode::Q => Some('Q'),
            Keycode::W => Some('W'),
            Keycode::E => Some('E'),
            Keycode::R => Some('R'),
            Keycode::T => Some('T'),
            Keycode::Y => Some('Y'),
            Keycode::U => Some('U'), 
            Keycode::I => Some('I'), 
            Keycode::O => Some('O'),
            Keycode::P => Some('P'),
            Keycode::LeftBracket => Some('{'),
            Keycode::RightBracket => Some('}'),
            Keycode::A => Some('A'),
            Keycode::S => Some('S'),
            Keycode::D => Some('D'),
            Keycode::F => Some('F'),
            Keycode::G => Some('G'),
            Keycode::H => Some('H'),
            Keycode::J => Some('J'),
            Keycode::K => Some('K'),
            Keycode::L => Some('L'),
            Keycode::Semicolon => Some(':'),
            Keycode::Quote => Some('"'), 
            Keycode::Backtick => Some('~'),
            Keycode::Backslash => Some('|'),
            Keycode::Z => Some('Z'),
            Keycode::X => Some('X'),
            Keycode::C => Some('C'),
            Keycode::V => Some('V'),
            Keycode::B => Some('B'),
            Keycode::N => Some('N'),
            Keycode::M => Some('M'),
            Keycode::Comma => Some('<'),
            Keycode::Period => Some('>'),
            Keycode::Slash => Some('?'),
            
            _ => self.as_ascii(),
        }
    }
}




// // I cant get TryFrom to work with core library
// use try_from::Err;

// #[derive(Debug)]
// pub struct TryFromKeycodeError { 
//     scan_code: u8,
// }

// impl Err for TryFromKeycodeError {
//     fn description(&self) -> &str {
//         "out of range integral type conversion attempted"
//     }
// }

// impl fmt::Display for TryFromKeycodeError {
//     fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
//         fmt.write_str(self.description())
//     }
// }

// impl TryFrom<u8> for Keycode {
//     type Err = TryFromKeycodeError;
//     fn try_from(original: u8) -> Result<Keycode, TryFromKeycodeError> {
//         let kc = get_keycode(original);
//         match kc {
//             Some(x) => Ok(x),
//             fail => Err(TryFromKeycodeError{ scan_code: original }),
//         }
//     }
// }
