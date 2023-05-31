cfg_if::cfg_if! {
    if #[cfg(target_arch = "x86_64")] {
        mod x86_64;
        pub(crate) use self::x86_64::*;
    } else {
        mod unsupported;
        pub(crate) use self::unsupported::*;
    }
}
