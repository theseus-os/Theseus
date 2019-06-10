#![no_std]

#[macro_use] extern crate log;
extern crate bit_field;
extern crate volatile;
extern crate memory;
extern crate network_interface_card;
extern crate pci;
extern crate spin;
extern crate mpmc;

pub mod descriptors;

use core::ops::DerefMut;
use memory::{EntryFlags, get_frame_allocator, PhysicalMemoryArea, FrameRange, PhysicalAddress, allocate_pages_by_bytes, get_kernel_mmi_ref, MappedPages, create_contiguous_mapping};
use pci::{pci_read_32, pci_write, PCI_BAR0, PciDevice, pci_set_command_bus_master_bit};
use spin::Once;
use network_interface_card::ReceiveBuffer;

/// Trait which contains functions for the NIC initialization procedure
pub trait NicInit {
    /// The mapping flags used for pages that the NIC will map.
    /// This should be a const, but Rust doesn't yet allow constants for the bitflags type
    fn nic_mapping_flags() -> EntryFlags {
        EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE
    }

    /// Returns the base address for the memory mapped registers
    fn determine_mem_base(dev: &PciDevice) -> Result<PhysicalAddress, &'static str> {
        // 64 bit address space
        let address_64 = 2;
        let bar0 = dev.bars[0];
        // 16-byte aligned memory mapped base address
        let mem_base = 
            // retrieve bits 1-2 to determine address space size
            if (bar0 >> 1) & 3 == address_64 { 
                // a 64-bit address so need to access BAR1 for the upper 32 bits
                let bar1 = dev.bars[1];
                PhysicalAddress::new((bar0 & !15) as usize | ((bar1 as usize) << 32))?
            }
            else {
                PhysicalAddress::new(bar0 as usize & !15)?
            };  
        
        debug!("mem_base: {:#X}", mem_base);

        Ok(mem_base)
    }

    /// Find out amount of space needed for device's registers
    /// TODO: Should this be placed in another crate? More generic than just a NIC function
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
        debug!("original bar0: {:#X}", bar0);
        mem_size
    }

    /// Allocates memory for the NIC registers, starting address and size taken from the PCI BAR0
    fn mem_map_reg(dev: &PciDevice, mem_base: PhysicalAddress) -> Result<MappedPages, &'static str> {
        // set the bus mastering bit for this PciDevice, which allows it to use DMA
        pci_set_command_bus_master_bit(dev);

        //find out amount of space needed
        let mem_size_in_bytes = Self::determine_mem_size(dev) as usize;

        Self::mem_map(mem_base, mem_size_in_bytes)
    }

    /// Allocates memory for the NIC
    fn mem_map(mem_base: PhysicalAddress, mem_size_in_bytes: usize) -> Result<MappedPages, &'static str> {
        // inform the frame allocator that the physical frames where the PCI config space for the nic exists
        // is now off-limits and should not be touched
        {
            let nic_area = PhysicalMemoryArea::new(mem_base, mem_size_in_bytes as usize, 1, 0); 
            get_frame_allocator().ok_or("ixgbe: Couldn't get FRAME ALLOCATOR")?.lock().add_area(nic_area, false)?;
        }

        // set up virtual pages and physical frames to be mapped
        let pages_nic = allocate_pages_by_bytes(mem_size_in_bytes).ok_or("ixgbe::mem_map(): couldn't allocated virtual page!")?;
        let frames_nic = FrameRange::from_phys_addr(mem_base, mem_size_in_bytes);
        let flags = Self::nic_mapping_flags();

        debug!("Ixgbe: memory base: {:#X}, memory size: {}", mem_base, mem_size_in_bytes);

        let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("ixgbe:mem_map KERNEL_MMI was not yet initialized!")?;
        let mut kernel_mmi = kernel_mmi_ref.lock();
        let mut fa = get_frame_allocator().ok_or("ixgbe::mem_map(): couldn't get FRAME_ALLOCATOR")?.lock();
        let nic_mapped_page = kernel_mmi.page_table.map_allocated_pages_to(pages_nic, frames_nic, flags, fa.deref_mut())?;

        Ok(nic_mapped_page)
    }

    fn init_rx_buf_pool(num_rx_buffers: usize, buffer_size: u16, rx_buffer_pool: &'static mpmc::Queue<ReceiveBuffer>) -> Result<(), &'static str> {
        let length = buffer_size;
        for _i in 0..num_rx_buffers {
            let (mp, phys_addr) = create_contiguous_mapping(length as usize, Self::nic_mapping_flags())?; 
            let rx_buf = ReceiveBuffer::new(mp, phys_addr, length, rx_buffer_pool);
            if rx_buffer_pool.push(rx_buf).is_err() {
                // if the queue is full, it returns an Err containing the object trying to be pushed
                error!("ixgbe::init_rx_buf_pool(): rx buffer pool is full, cannot add rx buffer {}!", _i);
                return Err("ixgbe rx buffer pool is full");
            };
        }

        Ok(())
    }

    // fn rx_queue_init() -> Result<(), &'static str>;
    // fn tx_queue_init() -> Result<(), &'static str>'
}