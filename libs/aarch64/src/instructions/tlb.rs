//! Functions to flush the translation lookaside buffer (TLB).

use VirtualAddress;

/// Invalidate the given address in the TLB using the `invlpg` instruction.
pub fn flush(_addr: VirtualAddress) {
    // TODO
}

/// Invalidate the TLB completely by reloading the CR3 register.
pub fn flush_all() {
    unsafe {
          asm!("
            isb;
            dsb ishst;
            tlbi vmalle1is;
            dsb nsh;            
            dsb ish; 
            isb; " : : : : "volatile");
         
    }
}
