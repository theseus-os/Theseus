//! Functions that are used in a NIC initialization procedure.
//! 
//! They include allocating memory space for the device's registers, and initializing its receive and transmit queues.

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate memory;
extern crate mpmc;
extern crate pci;
extern crate owning_ref;
extern crate intel_ethernet;
extern crate nic_buffers;
extern crate volatile;
extern crate nic_queues;

use memory::{EntryFlags, PhysicalAddress, allocate_pages_by_bytes, allocate_frames_by_bytes_at, get_kernel_mmi_ref, MappedPages, create_contiguous_mapping};
use pci::{PciDevice};
use alloc::{
    vec::Vec,
    boxed::Box,
};
use owning_ref::BoxRefMut;
use intel_ethernet::descriptors::{RxDescriptor, TxDescriptor};
use nic_buffers::ReceiveBuffer;
use nic_queues::{RxQueueRegisters, TxQueueRegisters};

/// The mapping flags used for pages that the NIC will map.
pub const NIC_MAPPING_FLAGS: EntryFlags = EntryFlags::from_bits_truncate(
    EntryFlags::PRESENT.bits() |
    EntryFlags::WRITABLE.bits() |
    EntryFlags::NO_CACHE.bits() |
    EntryFlags::NO_EXECUTE.bits()
);


/// Allocates memory for the NIC registers
/// 
/// # Arguments 
/// * `dev`: reference to pci device 
/// * `mem_base`: starting physical address of the device's memory mapped registers
pub fn allocate_device_register_memory(dev: &PciDevice, mem_base: PhysicalAddress) -> Result<MappedPages, &'static str> {
    //find out amount of space needed
    let mem_size_in_bytes = dev.determine_mem_size(0) as usize;

    allocate_memory(mem_base, mem_size_in_bytes)
}

/// Helper function to allocate memory at required address
/// 
/// # Arguments
/// * `mem_base`: starting physical address of the region that need to be allocated
/// * `mem_size_in_bytes`: size of the region that needs to be allocated 
pub fn allocate_memory(mem_base: PhysicalAddress, mem_size_in_bytes: usize) -> Result<MappedPages, &'static str> {
    // set up virtual pages and physical frames to be mapped
    let pages_nic = allocate_pages_by_bytes(mem_size_in_bytes)
        .ok_or("NicInit::mem_map(): couldn't allocate virtual page!")?;
    let frames_nic = allocate_frames_by_bytes_at(mem_base, mem_size_in_bytes)
        .map_err(|_e| "NicInit::mem_map(): couldn't allocate physical frames!")?;

    // debug!("NicInit: memory base: {:#X}, memory size: {}", mem_base, mem_size_in_bytes);

    let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("NicInit::mem_map(): KERNEL_MMI was not yet initialized!")?;
    let mut kernel_mmi = kernel_mmi_ref.lock();
    let nic_mapped_page = kernel_mmi.page_table.map_allocated_pages_to(pages_nic, frames_nic, NIC_MAPPING_FLAGS)?;

    Ok(nic_mapped_page)
}

/// Initialize the receive buffer pool from where receive buffers are taken and returned
/// 
/// # Arguments
/// * `num_rx_buffers`: number of buffers that are initially added to the pool 
/// * `buffer_size`: size of the receive buffers in bytes
/// * `rx_buffer_pool`: buffer pool to initialize
pub fn init_rx_buf_pool(num_rx_buffers: usize, buffer_size: u16, rx_buffer_pool: &'static mpmc::Queue<ReceiveBuffer>) -> Result<(), &'static str> {
    let length = buffer_size;
    for _i in 0..num_rx_buffers {
        let (mp, phys_addr) = create_contiguous_mapping(length as usize, NIC_MAPPING_FLAGS)?; 
        let rx_buf = ReceiveBuffer::new(mp, phys_addr, length, rx_buffer_pool);
        if rx_buffer_pool.push(rx_buf).is_err() {
            // if the queue is full, it returns an Err containing the object trying to be pushed
            error!("intel_ethernet::init_rx_buf_pool(): rx buffer pool is full, cannot add rx buffer {}!", _i);
            return Err("nic rx buffer pool is full");
        };
    }

    Ok(())
}

/// Steps to create and initialize a receive descriptor queue
/// 
/// # Arguments
/// * `num_desc`: number of descriptors in the queue
/// * `rx_buffer_pool`: pool from which to take receive buffers
/// * `buffer_size`: size of each buffer in the pool in bytes
/// * `rxq_regs`: registers needed to set up a receive queue 
pub fn init_rx_queue<T: RxDescriptor, S:RxQueueRegisters>(num_desc: usize, rx_buffer_pool: &'static mpmc::Queue<ReceiveBuffer>, buffer_size: usize, rxq_regs: &mut S)
    -> Result<(BoxRefMut<MappedPages, [T]>, Vec<ReceiveBuffer>), &'static str> 
{    
    let size_in_bytes_of_all_rx_descs_per_queue = num_desc * core::mem::size_of::<T>();
    
    // Rx descriptors must be 128 byte-aligned, which is satisfied below because it's aligned to a page boundary.
    let (rx_descs_mapped_pages, rx_descs_starting_phys_addr) = create_contiguous_mapping(size_in_bytes_of_all_rx_descs_per_queue, NIC_MAPPING_FLAGS)?;

    // cast our physically-contiguous MappedPages into a slice of receive descriptors
    let mut rx_descs = BoxRefMut::new(Box::new(rx_descs_mapped_pages)).try_map_mut(|mp| mp.as_slice_mut::<T>(0, num_desc))?;

    // now that we've created the rx descriptors, we can fill them in with initial values
    let mut rx_bufs_in_use: Vec<ReceiveBuffer> = Vec::with_capacity(num_desc);
    for rd in rx_descs.iter_mut()
    {
        // obtain or create a receive buffer for each rx_desc
        let rx_buf = rx_buffer_pool.pop()
            .ok_or("Couldn't obtain a ReceiveBuffer from the pool")
            .or_else(|_e| {
                create_contiguous_mapping(buffer_size, NIC_MAPPING_FLAGS)
                    .map(|(buf_mapped, buf_paddr)| 
                        ReceiveBuffer::new(buf_mapped, buf_paddr, buffer_size as u16, rx_buffer_pool)
                    )
            })?;
        let paddr_buf = rx_buf.phys_addr;
        rx_bufs_in_use.push(rx_buf); 


        rd.init(paddr_buf); 
    }

    // debug!("intel_ethernet::init_rx_queue(): phys_addr of rx_desc: {:#X}", rx_descs_starting_phys_addr);
    let rx_desc_phys_addr_lower  = rx_descs_starting_phys_addr.value() as u32;
    let rx_desc_phys_addr_higher = (rx_descs_starting_phys_addr.value() >> 32) as u32;
    
    // write the physical address of the rx descs ring
    rxq_regs.set_rdbal(rx_desc_phys_addr_lower);
    rxq_regs.set_rdbah(rx_desc_phys_addr_higher);

    // write the length (in total bytes) of the rx descs array
    rxq_regs.set_rdlen(size_in_bytes_of_all_rx_descs_per_queue as u32); // should be 128 byte aligned, minimum 8 descriptors
    
    // Write the head index (the first receive descriptor)
    rxq_regs.set_rdh(0);
    rxq_regs.set_rdt(0);   

    Ok((rx_descs, rx_bufs_in_use))        
}

/// Steps to create and initialize a transmit descriptor queue
/// 
/// # Arguments
/// * `num_desc`: number of descriptors in the queue
/// * `txq_regs`: registers needed to set up a transmit queue
pub fn init_tx_queue<T: TxDescriptor, S: TxQueueRegisters>(num_desc: usize, txq_regs: &mut S) 
    -> Result<BoxRefMut<MappedPages, [T]>, &'static str> 
{
    let size_in_bytes_of_all_tx_descs = num_desc * core::mem::size_of::<T>();
    
    // Tx descriptors must be 128 byte-aligned, which is satisfied below because it's aligned to a page boundary.
    let (tx_descs_mapped_pages, tx_descs_starting_phys_addr) = create_contiguous_mapping(size_in_bytes_of_all_tx_descs, NIC_MAPPING_FLAGS)?;

    // cast our physically-contiguous MappedPages into a slice of transmit descriptors
    let mut tx_descs = BoxRefMut::new(Box::new(tx_descs_mapped_pages))
        .try_map_mut(|mp| mp.as_slice_mut::<T>(0, num_desc))?;

    // now that we've created the tx descriptors, we can fill them in with initial values
    for td in tx_descs.iter_mut() {
        td.init();
    }

    // debug!("intel_ethernet::init_tx_queue(): phys_addr of tx_desc: {:#X}", tx_descs_starting_phys_addr);
    let tx_desc_phys_addr_lower  = tx_descs_starting_phys_addr.value() as u32;
    let tx_desc_phys_addr_higher = (tx_descs_starting_phys_addr.value() >> 32) as u32;

    // write the physical address of the tx descs array
    txq_regs.set_tdbal(tx_desc_phys_addr_lower); 
    txq_regs.set_tdbah(tx_desc_phys_addr_higher); 

    // write the length (in total bytes) of the tx descs array
    txq_regs.set_tdlen(size_in_bytes_of_all_tx_descs as u32);               
    
    // write the head index and the tail index (both 0 initially because there are no tx requests yet)
    txq_regs.set_tdh(0);
    txq_regs.set_tdt(0);

    Ok(tx_descs)
}

