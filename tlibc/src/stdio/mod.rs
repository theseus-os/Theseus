//! 

pub mod printf;
use crate::io::{StringWriter, UnsafeStringWriter, Write};

use core::ffi::VaList;
use alloc::{sync::Arc, vec::Vec};
use app_io::ImmutableWrite;
use libc::{c_char, c_int, size_t};


/// A `Write` adapter around the application's stdout stream.
///
/// `ImmutableWrite::write()` borrows `&self` because the stream does its own
/// locking internally, so there is no guard to hold here like the old
/// `stdout.lock()` call assumed — we just forward the bytes and let go.
struct StdoutWriter(Arc<dyn ImmutableWrite>);

impl Write for StdoutWriter {
    fn write(&mut self, buf: &[u8]) -> core2::io::Result<usize> {
        self.0.write(buf)
    }
    fn write_all(&mut self, buf: &[u8]) -> core2::io::Result<()> {
        self.0.write_all(buf)
    }
    fn flush(&mut self) -> core2::io::Result<()> {
        // Nothing is buffered on this side of the stream, so there's nothing to push.
        Ok(())
    }
}


// #[no_mangle]
// pub unsafe extern "C" fn vfprintf(file: *mut FILE, format: *const c_char, ap: VaList) -> c_int {
//     let mut file = (*file).lock();
//     if let Err(_) = file.try_set_byte_orientation_unlocked() {
//         return -1;
//     }

//     printf::printf(&mut *file, format, ap)
// }

#[no_mangle]
pub unsafe extern "C" fn vprintf(format: *const c_char, ap: VaList) -> c_int {
    let stream = match app_io::stdout() {
        Ok(stream) => stream,
        Err(e) => {
            // No stdout stream for this task; report the failure to the caller
            // instead of panicking inside libc.
            warn!("vprintf: couldn't get the stdout stream: {e}");
            return -1;
        }
    };
    let mut writer = StdoutWriter(stream);
    printf::printf(&mut writer, format, ap)
}

#[no_mangle]
pub unsafe extern "C" fn vasprintf(
    strp: *mut *mut c_char,
    format: *const c_char,
    ap: VaList,
) -> c_int {
    let mut alloc_writer = Vec::new();
    let ret = printf::printf(&mut alloc_writer, format, ap);
    alloc_writer.push(0); // null terminated
    alloc_writer.shrink_to_fit();
    *strp = alloc_writer.leak().as_ptr() as *mut c_char;
    ret
}

#[no_mangle]
pub unsafe extern "C" fn vsnprintf(
    s: *mut c_char,
    n: size_t,
    format: *const c_char,
    ap: VaList,
) -> c_int {
    printf::printf(
        &mut StringWriter(s as *mut u8, n as usize),
        format,
        ap,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vsprintf(s: *mut c_char, format: *const c_char, ap: VaList) -> c_int {
    printf::printf(&mut UnsafeStringWriter(s as *mut u8), format, ap)
}
