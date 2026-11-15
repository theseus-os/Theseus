//! Structs which provide access to the ixgbe device queue registers and store their backing memory pages.
//! 
//! They implement the `RxQueueRegisters` and `TxQueueRegisters` traits which allows 
//! the registers to be accessed through virtual NICs

use super::regs::{RegistersRx, RegistersTx};
use alloc::sync::Arc;
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;
use nic_queues::{RxQueueRegisters, TxQueueRegisters};
use memory::MappedPages;


/// A non-null pointer to a register block that lives in MMIO memory,
/// i.e. memory that was mapped from one of the NIC's BARs.
///
/// We deliberately do *not* use a `Box` for this: the pointed-to memory comes
/// from a `MappedPages` mapping, not from the heap, so a `Box` would be invalid
/// from the start (the global allocator never handed out this memory) and
/// dropping it would run the deallocator on MMIO memory.
/// A plain pointer has no destructor at all, which also closes the window where
/// an unwinding task could have freed the registers by mistake.
#[derive(Debug)]
pub struct MmioPtr<T>(NonNull<T>);

impl<T> MmioPtr<T> {
    /// Creates a pointer from the raw address of a mapped register block.
    ///
    /// # Safety
    /// `ptr` must be non-null, properly aligned for `T`, and must stay mapped
    /// for as long as the returned pointer is used. The usual way to guarantee
    /// the last part is to keep the owning `MappedPages` alive alongside it.
    pub unsafe fn new(ptr: *mut T) -> Self {
        Self(NonNull::new(ptr).expect("null pointer passed to MmioPtr::new"))
    }
}

impl<T> Deref for MmioPtr<T> {
    type Target = T;
    fn deref(&self) -> &T {
        // Safety: guaranteed by `new()` and by whoever owns this pointer,
        // which also holds the `MappedPages` backing the mapping.
        unsafe { self.0.as_ref() }
    }
}

impl<T> DerefMut for MmioPtr<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { self.0.as_mut() }
    }
}

// A `MmioPtr` is only a handle to memory owned elsewhere, so it can be sent or
// shared exactly when the pointee can be, just like the `Box` it replaces.
unsafe impl<T: Send> Send for MmioPtr<T> {}
unsafe impl<T: Sync> Sync for MmioPtr<T> {}


/// Struct that stores a pointer to registers for one ixgbe receive queue
/// as well as a shared reference to the backing `MappedPages` where these registers are located.
pub struct IxgbeRxQueueRegisters {
    /// The registers themselves, living inside `backing_pages`.
    /// Nothing here is ever dropped or freed; the mapping is torn down only
    /// when the last `backing_pages` reference goes away.
    pub regs: MmioPtr<RegistersRx>,
    pub backing_pages: Arc<MappedPages>
}

impl RxQueueRegisters for IxgbeRxQueueRegisters {
    fn set_rdbal(&mut self, value: u32) {
        self.regs.rdbal.write(value)
    }    
    fn set_rdbah(&mut self, value: u32) {
        self.regs.rdbah.write(value)
    }
    fn set_rdlen(&mut self, value: u32) {
        self.regs.rdlen.write(value)
    }
    fn set_rdh(&mut self, value: u32) {
        self.regs.rdh.write(value)
    }
    fn set_rdt(&mut self, value: u32) {
        self.regs.rdt.write(value)
    }
}
impl Deref for IxgbeRxQueueRegisters {
    type Target = RegistersRx;
    fn deref(&self) -> &RegistersRx {
        &self.regs
    }
}
impl DerefMut for IxgbeRxQueueRegisters {
    fn deref_mut(&mut self) -> &mut RegistersRx {
        &mut self.regs
    }
}

/// Struct that stores a pointer to registers for one ixgbe transmit queue
/// as well as a shared reference to the backing `MappedPages` where these registers are located.
pub struct IxgbeTxQueueRegisters {
    /// The registers themselves, living inside `backing_pages`.
    /// Nothing here is ever dropped or freed; the mapping is torn down only
    /// when the last `backing_pages` reference goes away.
    pub regs: MmioPtr<RegistersTx>,
    pub backing_pages: Arc<MappedPages>
}
impl TxQueueRegisters for IxgbeTxQueueRegisters {
    fn set_tdbal(&mut self, value: u32) {
        self.regs.tdbal.write(value)
    }  
    fn set_tdbah(&mut self, value: u32) {
        self.regs.tdbah.write(value)
    }
    fn set_tdlen(&mut self, value: u32) {
        self.regs.tdlen.write(value)
    }
    fn set_tdh(&mut self, value: u32) {
        self.regs.tdh.write(value)
    }
    fn set_tdt(&mut self, value: u32) {
        self.regs.tdt.write(value)
    }
}
impl Deref for IxgbeTxQueueRegisters {
    type Target = RegistersTx;
    fn deref(&self) -> &RegistersTx {
        &self.regs
    }
}
impl DerefMut for IxgbeTxQueueRegisters {
    fn deref_mut(&mut self) -> &mut RegistersTx {
        &mut self.regs
    }
}
