use bit_field::BitField;
use volatile::{Volatile, ReadOnly};
use core::ops::DerefMut;
use memory::{FRAME_ALLOCATOR, EntryFlags, PhysicalMemoryArea, FrameRange, PhysicalAddress, VirtualAddress, allocate_pages_by_bytes, get_kernel_mmi_ref, MappedPages, create_contiguous_mapping};
use pci::{pci_read_32, pci_write, PCI_BAR0, PciDevice, pci_set_command_bus_master_bit};
use spin::Once;
use alloc::{
    vec::Vec,
    boxed::Box,
};
use owning_ref::BoxRefMut;
use core::ptr::write_volatile;
use alloc::collections::VecDeque;
use irq_safety::MutexIrqSafe;
use intel_descriptors::{RxDescriptor, TxDescriptor};
use nic_buffers::{TransmitBuffer, ReceiveBuffer, ReceivedFrame};
use super::nic_mapping_flags;





/// Trait which contains functions for the NIC initialization procedure
pub trait NicInit {

    /// Returns the base address for the memory mapped registers from Base Address Register 0.
    /// 
    /// # Arguments
    /// * `dev`: pci device we need to find the base address for
    fn determine_mem_base(dev: &PciDevice) -> Result<PhysicalAddress, &'static str> {
        // value in the BAR which means a 64-bit address space
        let address_64 = 2;
        let bar0 = dev.bars[0];

        // memory mapped base address
        let mem_base = 
            // retrieve bits 1-2 to determine address space size
            if (bar0 >> 1) & 3 == address_64 { 
                // a 64-bit address so need to access BAR1 for the upper 32 bits
                let bar1 = dev.bars[1];
                // clear out the bottom 4 bits because it's a 16-byte aligned address
                PhysicalAddress::new((bar0 & !15) as usize | ((bar1 as usize) << 32))?
            }
            else {
                // clear out the bottom 4 bits because it's a 16-byte aligned address
                PhysicalAddress::new(bar0 as usize & !15)?
            };  
        
        Ok(mem_base)
    }

    /// Find out amount of space needed for device's registers
    /// 
    /// # Arguments
    /// * `dev`: pci device we need to find the memory size for
    fn determine_mem_size(dev: &PciDevice) -> u32 {
        // Here's what we do: 
        // 1) read pci reg
        // 2) bitwise not and add 1 because ....
        pci_write(dev.bus, dev.slot, dev.func, PCI_BAR0, 0xFFFF_FFFF);
        let mut mem_size = pci_read_32(dev.bus, dev.slot, dev.func, PCI_BAR0);
        //debug!("mem_size_read: {:x}", mem_size);
        mem_size = mem_size & 0xFFFF_FFF0; //mask info bits
        mem_size = !(mem_size); //bitwise not
        //debug!("mem_size_read_not: {:x}", mem_size);
        mem_size = mem_size +1; // add 1
        //debug!("mem_size: {}", mem_size);
        pci_write(dev.bus, dev.slot, dev.func, PCI_BAR0, dev.bars[0]); //restore original value
        //check that value is restored
        let bar0 = pci_read_32(dev.bus, dev.slot, dev.func, PCI_BAR0);
        // debug!("original bar0: {:#X}", bar0);
        mem_size
    }

    /// Allocates memory for the NIC registers
    /// # Arguments 
    /// * `dev`: reference to pci device 
    /// * `mem_base`: starting physical address of the device's memory mapped registers
    fn mem_map_reg(dev: &PciDevice, mem_base: PhysicalAddress) -> Result<MappedPages, &'static str> {
        // set the bus mastering bit for this PciDevice, which allows it to use DMA
        pci_set_command_bus_master_bit(dev);

        //find out amount of space needed
        let mem_size_in_bytes = Self::determine_mem_size(dev) as usize;

        Self::mem_map(mem_base, mem_size_in_bytes)
    }

    /// Helper function to allocate memory
    /// 
    /// # Arguments
    /// * `mem_base`: starting physical address of the region that need to be allocated
    /// * `mem_size_in_bytes`: size of the region that needs to be allocated 
    fn mem_map(mem_base: PhysicalAddress, mem_size_in_bytes: usize) -> Result<MappedPages, &'static str> {
        // inform the frame allocator that the physical frames where memory area for the nic exists
        // is now off-limits and should not be touched
        {
            let nic_area = PhysicalMemoryArea::new(mem_base, mem_size_in_bytes as usize, 1, 0); 
            FRAME_ALLOCATOR.try().ok_or("NicInit::mem_map(): Couldn't get FRAME ALLOCATOR")?.lock().add_area(nic_area, false)?;
        }

        // set up virtual pages and physical frames to be mapped
        let pages_nic = allocate_pages_by_bytes(mem_size_in_bytes).ok_or("NicInit::mem_map(): couldn't allocated virtual page!")?;
        let frames_nic = FrameRange::from_phys_addr(mem_base, mem_size_in_bytes);
        let flags = nic_mapping_flags();

        // debug!("NicInit: memory base: {:#X}, memory size: {}", mem_base, mem_size_in_bytes);

        let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("NicInit::mem_map(): KERNEL_MMI was not yet initialized!")?;
        let mut kernel_mmi = kernel_mmi_ref.lock();
        let mut fa = FRAME_ALLOCATOR.try().ok_or("NicInit::mem_map(): Couldn't get FRAME ALLOCATOR")?.lock();
        let nic_mapped_page = kernel_mmi.page_table.map_allocated_pages_to(pages_nic, frames_nic, flags, fa.deref_mut())?;

        Ok(nic_mapped_page)
    }

    /// Initialize the receive buffer pool from where receive buffers are taken and returned
    /// 
    /// # Arguments
    /// * `num_rx_buffers`: number of buffers that are initially added to the pool 
    /// * `buffer_size`: size of the receive buffers in bytes
    /// * `rx_buffer_pool: buffer pool to initialize
    fn init_rx_buf_pool(num_rx_buffers: usize, buffer_size: u16, rx_buffer_pool: &'static mpmc::Queue<ReceiveBuffer>) -> Result<(), &'static str> {
        let length = buffer_size;
        for _i in 0..num_rx_buffers {
            let (mp, phys_addr) = create_contiguous_mapping(length as usize, nic_mapping_flags())?; 
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
    /// * `rx_buffer_pool`: tpool from which to take receive buffers
    /// * `buffer_size`: size of each buffer in the pool
    /// * `rdbal`: register which stores the lower 32 bits of the buffer physical address
    /// * `rdbah`: register which stores the higher 32 bits of the buffer physical address
    /// * `rdlen`: register which stores the length of the queue in bytes
    /// * `rdh`: register which stores the descriptor at the head of the queue
    /// * `rdt`: register which stores the descriptor at the tail of the queue
    fn init_rx_queue<T: RxDescriptor>(num_desc: usize, rx_buffer_pool: &'static mpmc::Queue<ReceiveBuffer>, buffer_size: usize,
        rdbal: &mut Volatile<u32>, rdbah: &mut Volatile<u32>, rdlen: &mut Volatile<u32>, rdh: &mut Volatile<u32>, rdt: &mut Volatile<u32>) 
        -> Result<(BoxRefMut<MappedPages, [T]>, Vec<ReceiveBuffer>), &'static str> {
        
        let size_in_bytes_of_all_rx_descs_per_queue = num_desc * core::mem::size_of::<T>();
     
        // Rx descriptors must be 128 byte-aligned, which is satisfied below because it's aligned to a page boundary.
        let (rx_descs_mapped_pages, rx_descs_starting_phys_addr) = create_contiguous_mapping(size_in_bytes_of_all_rx_descs_per_queue, nic_mapping_flags())?;

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
                    create_contiguous_mapping(buffer_size, nic_mapping_flags())
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
        rdbal.write(rx_desc_phys_addr_lower);
        rdbah.write(rx_desc_phys_addr_higher);

        // write the length (in total bytes) of the rx descs array
        rdlen.write(size_in_bytes_of_all_rx_descs_per_queue as u32); // should be 128 byte aligned, minimum 8 descriptors
        
        // Write the head index (the first receive descriptor)
        rdh.write(0);
        rdt.write(0);   

        Ok((rx_descs, rx_bufs_in_use))        
    }

    /// Steps to create and initialize a transmit descriptor queue
    /// 
    /// # Arguments
    /// * `num_desc`: number of descriptors in the queue
    /// * `tdbal`: register which stores the lower 32 bits of the buffer physical address
    /// * `tdbah`: register which stores the higher 32 bits of the buffer physical address
    /// * `tdlen`: register which stores the length of the queue in bytes
    /// * `tdh`: register which stores the descriptor at the head of the queue
    /// * `tdt`: register which stores the descriptor at the tail of the queue
    fn init_tx_queue<T: TxDescriptor>(num_desc: usize, tdbal: &mut Volatile<u32>, tdbah: &mut Volatile<u32>, tdlen: &mut Volatile<u32>, tdh: &mut Volatile<u32>, tdt: &mut Volatile<u32>) 
        -> Result<BoxRefMut<MappedPages, [T]>, &'static str> {

        let size_in_bytes_of_all_tx_descs = num_desc * core::mem::size_of::<T>();
        
        // Tx descriptors must be 128 byte-aligned, which is satisfied below because it's aligned to a page boundary.
        let (tx_descs_mapped_pages, tx_descs_starting_phys_addr) = create_contiguous_mapping(size_in_bytes_of_all_tx_descs, nic_mapping_flags())?;

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
        tdbal.write(tx_desc_phys_addr_lower); 
        tdbah.write(tx_desc_phys_addr_higher); 

        // write the length (in total bytes) of the tx descs array
        tdlen.write(size_in_bytes_of_all_tx_descs as u32);               
        
        // write the head index and the tail index (both 0 initially because there are no tx requests yet)
        tdh.write(0);
        tdt.write(0);

        Ok(tx_descs)
    }
}