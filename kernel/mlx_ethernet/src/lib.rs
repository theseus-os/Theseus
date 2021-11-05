//! This crate defines the layout of memory objects that make up the software interface between the Mellanox hardware and the driver,
//! as well as functions to access different fields of these objects.
//!
//! The Mellanox ethernet card is referred to as both the NIC (Network Interface Card) and the HCA (Host Channel Adapter).
//!
//! All information is taken from the Mellanox Adapters Programmer’s Reference Manual (PRM) [Rev 0.54], unless otherwise specified.
//! While this version of the manual was acquired by directly contacting Nvidia through their support site (<https://support.mellanox.com/s/>),
//! an older version of the manual can be found at <http://www.mellanox.com/related-docs/user_manuals/Ethernet_Adapters_Programming_Manual.pdf>.

 #![no_std]
 #![feature(slice_pattern)]
 #![feature(core_intrinsics)]

#[macro_use]extern crate log;
#[macro_use] extern crate alloc;
#[macro_use] extern crate static_assertions;
extern crate memory;
extern crate volatile;
extern crate bit_field;
extern crate zerocopy;
extern crate owning_ref;
extern crate byteorder;
extern crate nic_initialization;
extern crate kernel_config;
extern crate libm;
extern crate num_enum;

use kernel_config::memory::PAGE_SIZE;

pub mod initialization_segment;
pub mod command_queue;
pub mod event_queue;
pub mod completion_queue;
pub mod send_queue;
pub mod receive_queue;
pub mod work_queue;
mod uar;

const UAR_MASK:                 u32 = 0xFF_FFFF;
const LOG_QUEUE_SIZE_MASK:      u32 = 0x1F;
const CQN_MASK:                 u32 = 0xFF_FFFF;
const LOG_QUEUE_SIZE_SHIFT:     u32 = 24;
const LOG_PAGE_SIZE_SHIFT:      u32 = 24;

const HW_OWNERSHIP:             u32 = 1;

/// Find the page size of the given `num_bytes` in units of 4KiB pages.
pub fn log_page_size(num_bytes: u32) -> u32 {
    libm::log2(libm::ceil(num_bytes as f64 / PAGE_SIZE as f64)) as u32
}