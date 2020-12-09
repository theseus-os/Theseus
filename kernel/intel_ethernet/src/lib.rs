//! Definitions of descriptor types and type aliases for NIC registers that are used in Intel ethernet card drivers
//! 
//! Descriptors are used for DMA by the NIC hardware, and for communication between the driver SW and NIC HW.
//! Descriptors that can be used in Intel NIC drivers are defined here. 
//! There are multiple types, with newer NICs using advanced descriptors but still retaining support for legacy descriptors.
//! The physical memory address where packets are located are written to the descriptors as well as other 
//! information bits needed for proper communication between SW and HW.
//! More information about descriptors can be found from Intel NIC datasheets.
//! 
//! Type aliases for some NIC registers that are common across different Intel NICs are also defined here.
 

#![no_std]

// #[macro_use]extern crate log;
extern crate memory;
extern crate volatile;
extern crate bit_field;
extern crate zerocopy;

pub mod descriptors;