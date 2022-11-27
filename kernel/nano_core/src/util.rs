/// Just like Rust's `try!()` macro, but instead of performing an early return
/// upon an error, it invokes the `shutdown()` function upon an error in order
/// to cleanly exit Theseus OS.
#[macro_export]
macro_rules! try_exit {
    ($expr:expr) => {
        match $expr {
            Ok(val) => val,
            Err(err_msg) => {
                $crate::util::shutdown(format_args!("{}", err_msg));
            }
        }
    };
}

/// Shuts down Theseus and prints the given formatted arguuments.
pub(crate) fn shutdown(msg: core::fmt::Arguments) -> ! {
    // println_raw!("Theseus is shutting down, msg: {}", msg);
    log::error!("Theseus is shutting down, msg: {}", msg);

    // TODO: handle shutdowns properly with ACPI commands
    panic!("{}", msg);
}

// FIXME: This shouldn't be necessary
#[macro_export]
macro_rules! println_raw {
    ($($tail:tt)*) => {
        #[cfg(feature = "bios")]
        vga_buffer::println_raw!($($tail)*);
    }
}
