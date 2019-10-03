//! ctype implementation for Redox, following http://pubs.opengroup.org/onlinepubs/7908799/xsh/ctype.h.html

use crate::types::*;

#[no_mangle]
pub extern "C" fn isalnum(c: c_int) -> c_int {
    (isdigit(c) != 0 || isalpha(c) != 0) as c_int
}

#[no_mangle]
pub extern "C" fn isalpha(c: c_int) -> c_int {
    (islower(c) != 0 || isupper(c) != 0) as c_int
}

#[no_mangle]
pub extern "C" fn isascii(c: c_int) -> c_int {
    ((c & !0x7f) == 0) as c_int
}

#[no_mangle]
pub extern "C" fn isblank(c: c_int) -> c_int {
    (c == ' ' as c_int || c == '\t' as c_int) as c_int
}

#[no_mangle]
pub extern "C" fn iscntrl(c: c_int) -> c_int {
    ((c >= 0x00 && c <= 0x1f) || c == 0x7f) as c_int
}

#[no_mangle]
pub extern "C" fn isdigit(c: c_int) -> c_int {
    (c >= b'0' as c_int && c <= b'9' as c_int) as c_int
}

#[no_mangle]
pub extern "C" fn isgraph(c: c_int) -> c_int {
    (c >= 0x21 && c <= 0x7e) as c_int
}

#[no_mangle]
pub extern "C" fn islower(c: c_int) -> c_int {
    (c >= b'a' as c_int && c <= b'z' as c_int) as c_int
}

#[no_mangle]
pub extern "C" fn isprint(c: c_int) -> c_int {
    (c >= 0x20 && c < 0x7f) as c_int
}

#[no_mangle]
pub extern "C" fn ispunct(c: c_int) -> c_int {
    ((c >= b'!' as c_int && c <= b'/' as c_int)
        || (c >= b':' as c_int && c <= b'@' as c_int)
        || (c >= b'[' as c_int && c <= b'`' as c_int)
        || (c >= b'{' as c_int && c <= b'~' as c_int)) as c_int
}

#[no_mangle]
pub extern "C" fn isspace(c: c_int) -> c_int {
    (c == ' ' as c_int
        || c == '\t' as c_int
        || c == '\n' as c_int
        || c == '\r' as c_int
        || c == 0x0b
        || c == 0x0c) as c_int
}

#[no_mangle]
pub extern "C" fn isupper(c: c_int) -> c_int {
    (c >= b'A' as c_int && c <= b'Z' as c_int) as c_int
}

#[no_mangle]
pub extern "C" fn isxdigit(c: c_int) -> c_int {
    (isdigit(c) != 0 || (c | 32 >= b'a' as c_int && c | 32 <= 'f' as c_int)) as c_int
}

#[no_mangle]
/// The comment in musl:
/// "nonsense function that should NEVER be used!"
pub extern "C" fn toascii(c: c_int) -> c_int {
    c & 0x7f
}

#[no_mangle]
pub extern "C" fn tolower(c: c_int) -> c_int {
    if isupper(c) != 0 {
        c | 0x20
    } else {
        c
    }
}

#[no_mangle]
pub extern "C" fn toupper(c: c_int) -> c_int {
    if islower(c) != 0 {
        c & !0x20
    } else {
        c
    }
}
