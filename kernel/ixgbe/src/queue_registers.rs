//! Structs which provide access to the ixgbe device queue registers and store their backing memory pages.
//! They implement the `RxQueueRegisters` and `TxQueueRegisters` traits which allows 
//! the registers to be accessed through virtual NICs

use super::regs::{RegistersRx, RegistersTx};
use alloc::{
    sync::Arc,
    boxed::Box
};
use core::ops::{Deref, DerefMut};
use core::mem::ManuallyDrop;
use nic_queues::{RxQueueRegisters, TxQueueRegisters};
use memory::MappedPages;


/// Struct that stores a pointer to registers for one ixgbe receive queue 
/// as well as a shared reference to the backing MappedPages where these registers are located.
pub struct IxgbeRxQueueRegisters {
    /// We prevent the drop handler from dropping the `regs` because the backing memory is not on the heap, but in the stored mapped pages.
    /// The memory will be deallocated when the `backing_pages` are dropped.
    pub regs: ManuallyDrop<Box<RegistersRx>>,
    pub backing_pages: Arc<MappedPages>
}

impl RxQueueRegisters for IxgbeRxQueueRegisters {
    fn update_rdbal(&mut self, value: u32) {
        self.regs.rdbal.write(value)
    }    
    fn update_rdbah(&mut self, value: u32) {
        self.regs.rdbah.write(value)
    }
    fn update_rdlen(&mut self, value: u32) {
        self.regs.rdlen.write(value)
    }
    fn update_rdh(&mut self, value: u32) {
        self.regs.rdh.write(value)
    }
    fn update_rdt(&mut self, value: u32) {
        self.regs.rdt.write(value)
    }
}
impl Deref for IxgbeRxQueueRegisters {
    type Target = Box<RegistersRx>;
    fn deref(&self) -> &Box<RegistersRx> {
        &self.regs
    }
}
impl DerefMut for IxgbeRxQueueRegisters {
    fn deref_mut(&mut self) -> &mut Box<RegistersRx> {
        &mut self.regs
    }
}

/// Struct that stores a pointer to registers for one ixgbe transmit queue 
/// as well as a shared reference to the backing MappedPages where these registers are located.
pub struct IxgbeTxQueueRegisters {
    /// We prevent the drop handler from dropping the `regs` because the backing memory is not on the heap, but in the stored mapped pages.
    /// The memory will be deallocated when the `backing_pages` are dropped.
    pub regs: ManuallyDrop<Box<RegistersTx>>,
    pub backing_pages: Arc<MappedPages>
}
impl TxQueueRegisters for IxgbeTxQueueRegisters {
    fn update_tdbal(&mut self, value: u32) {
        self.regs.tdbal.write(value)
    }  
    fn update_tdbah(&mut self, value: u32) {
        self.regs.tdbah.write(value)
    }
    fn update_tdlen(&mut self, value: u32) {
        self.regs.tdlen.write(value)
    }
    fn update_tdh(&mut self, value: u32) {
        self.regs.tdh.write(value)
    }
    fn update_tdt(&mut self, value: u32) {
        self.regs.tdt.write(value)
    }
}
impl Deref for IxgbeTxQueueRegisters {
    type Target = Box<RegistersTx>;
    fn deref(&self) -> &Box<RegistersTx> {
        &self.regs
    }
}
impl DerefMut for IxgbeTxQueueRegisters {
    fn deref_mut(&mut self) -> &mut Box<RegistersTx> {
        &mut self.regs
    }
}
