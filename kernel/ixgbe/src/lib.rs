#![no_std]
#![feature(alloc)]
#![feature(untagged_unions)]
#![allow(dead_code)] //  to suppress warnings for unused functions/methods
#![allow(safe_packed_borrows)] // temporary, just to suppress unsafe packed borrows 


#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
extern crate alloc;
extern crate spin;
extern crate irq_safety;
extern crate kernel_config;
extern crate memory;
extern crate pci; 
extern crate pit_clock; 


pub mod test_e1000e_driver;

use core::ptr::{read_volatile, write_volatile};
use core::ops::DerefMut;
use spin::Once;
use alloc::Vec;
use irq_safety::MutexIrqSafe;
use alloc::boxed::Box;
use memory::{get_kernel_mmi_ref,FRAME_ALLOCATOR, MemoryManagementInfo, PhysicalAddress, Frame, PageTable, EntryFlags, FrameAllocator, allocate_pages, MappedPages,FrameIter,PhysicalMemoryArea};
use pci::{PciDevice,pci_read_32, pci_read_8, pci_write, get_pci_device_vd, pci_set_command_bus_master_bit};
use kernel_config::memory::PAGE_SIZE;



