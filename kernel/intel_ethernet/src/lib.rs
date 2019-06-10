#![no_std]

#[macro_use] extern crate log;
extern crate bit_field;
extern crate volatile;
extern crate memory;
extern crate network_interface_card;

pub mod descriptors;

pub trait NicInitialization {
    // fn nic_mapping_flags() -> EntryFlags
    // fn determine_mem_size(dev: &PciDevice) -> u32 ;
    // fn mem_map (dev: &PciDevice, mem_base: PhysicalAddress) -> Result<(BoxRefMut<MappedPages, IntelIxgbeRegisters>, VirtualAddress), &'static str>;
    // fn init_rx_buf_pool(num_rx_buffers: usize) -> Result<(), &'static str>;
    // pub fn spoof_mac(&mut self, spoofed_mac_addr: [u8; 6]);
    // fn rx_queue_init() -> Result<(), &'static str>;
    // fn tx_queue_init() -> Result<(), &'static str>'
}