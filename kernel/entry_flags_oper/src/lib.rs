#![no_std]

/// Basic operations on entryflags
pub trait EntryFlagsOper<T> {
    /// Returns ture if the page the entry points to is a huge page. 
    /// Which means the flags contains a HUGE_PAGE bit
    fn is_huge(&self) -> bool;

    /// The default flags of an accessible page. 
    /// For every accessiable page the PRESENT bit should be set
    fn default_flags() -> T;

    /// return the flags of a writable page excluding the default bits
    fn writable() -> T;

    /// The flags of a writable page. 
    /// For every writable page the PRESENT and WRITABLE bits should be set
    fn rw_flags() -> T;

    /// Returns true if the page is accessiable and is not huge
    fn is_page(&self) -> bool;

    /// Set the page the entry points to as writable.
    /// Set the PRESENT and WRITABLE bits of the flags
    fn set_writable(&self) -> T;

    /// Returns true if these flags have the `WRITABLE` bit set.
    fn is_writable(&self) -> bool;

    /// Returns true if these flags are executable, 
    /// which means that the `NO_EXECUTE` bit on x86 is *not* set.
    fn is_executable(&self) -> bool;
}