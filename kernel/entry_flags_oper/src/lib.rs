//! The `EntrFlagsOper` trait defines basic operations on entry flags. The `mmu` crate implements this trait for `EntryFlags` on different architectures.

#![no_std]

/// Basic operations on entryflags
pub trait EntryFlagsOper<T> {
    /// Returns ture if the page the entry points to is a huge page. 
    /// For x86, it means the flags contain a `HUGE_PAGE` bit.
    fn is_huge(&self) -> bool;

    /// The default flags of an accessible page. 
    /// For x86, the `PRESENT` bit should be set.
    fn default_flags() -> T;

    /// return the flags of a writable page excluding the default bits.
    fn writable() -> T;

    /// The flags of a writable page. 
    /// For x86 the `PRESENT` and `WRITABLE` bits should be set.
    fn rw_flags() -> T;

    /// Returns true if the page is accessiable and is not huge.
    fn is_page(&self) -> bool;

    /// Set the page the entry points to as writable. 
    /// For x86 set the `PRESENT` and `WRITABLE` bits of the flags.
    fn set_writable(&self) -> T;

    /// Returns true if the page is writable. 
    /// For x86 it means the flags contain `WRITABLE`.
    fn is_writable(&self) -> bool;

    /// Returns true if these flags are executable,
    /// which means that the `NO_EXECUTE` bit on x86 is *not* set.
    fn is_executable(&self) -> bool;
}
