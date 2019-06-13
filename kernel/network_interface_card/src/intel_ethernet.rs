use bit_field::BitField;
use volatile::{Volatile, ReadOnly};
use core::ops::DerefMut;
use memory::{EntryFlags, get_frame_allocator, PhysicalMemoryArea, FrameRange, PhysicalAddress, VirtualAddress, allocate_pages_by_bytes, get_kernel_mmi_ref, MappedPages, create_contiguous_mapping};
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

use super::{TransmitBuffer, ReceiveBuffer, nic_mapping_flags, ReceivedFrame};

/// A trait for functionalities that all receive descriptors must support
pub trait RxDescriptor {
    /// Initializes a receive descriptor by clearing its status 
    /// and setting the descriptor's physical address.
    /// 
    /// # Arguments
    /// * `packet_buffer_address`: the starting physical address of the receive buffer.
    fn init(&mut self, packet_buffer_address: PhysicalAddress);

    /// Updates the descriptor's physical address.
    /// 
    /// # Arguments
    /// * `packet_buffer_address`: the starting physical address of the receive buffer.
    fn set_packet_address(&mut self, packet_buffer_address: PhysicalAddress);

    /// Clears the status bits of the descriptor.
    fn reset_status(&mut self);

    /// Returns true if the descriptor has a received packet copied to its buffer.
    fn descriptor_done(&self) -> bool;

    /// Returns true if the descriptor's packet buffer is the last in a frame.
    fn end_of_packet(&self) -> bool;

    /// The length of the packet in the descriptor's packet buffer.
    fn length(&self) -> u64;
}

/// A trait for functionalities that all transmit descriptors must support.
pub trait TxDescriptor {
    /// Initializes a transmit descriptor by clearing all of its values.
    fn init(&mut self);

    /// Updates the transmit descriptor to send the packet.
    /// 
    /// # Arguments
    /// * `transmit_buffer`: the buffer which contains the packet to be sent. We assume that one transmit descriptor will be used to send one packet.
    fn send(&mut self, transmit_buffer: TransmitBuffer);

    /// Polls the Descriptor Done bit until the packet has been sent.
    fn wait_for_packet_tx(&self);
}



/// This struct is a Legacy Transmit Descriptor. 
/// There is one instance of this struct per transmit buffer. 
#[repr(C,packed)]
pub struct LegacyTxDesc {
    /// The starting physical address of the transmit buffer
    pub phys_addr:  Volatile<u64>,
    /// Length of the transmit buffer in bytes
    pub length:     Volatile<u16>,
    /// Checksum offset: where to insert the checksum from the start of the packet if enabled
    pub cso:        Volatile<u8>,
    /// Command bits
    pub cmd:        Volatile<u8>,
    /// Status bits
    pub status:     Volatile<u8>,
    /// Checksum start: where to begin computing the checksum, if enabled
    pub css:        Volatile<u8>,
    /// Vlan tags 
    pub vlan :   Volatile<u16>,
}

impl TxDescriptor for LegacyTxDesc {
    fn init(&mut self) {
        self.phys_addr.write(0);
        self.length.write(0);
        self.cso.write(0);
        self.cmd.write(0);
        self.status.write(0);
        self.css.write(0);
        self.vlan.write(0);
    }

    fn send(&mut self, transmit_buffer: TransmitBuffer) {
        self.phys_addr.write(transmit_buffer.phys_addr.value() as u64);
        self.length.write(transmit_buffer.length);
        self.cmd.write(TX_CMD_EOP | TX_CMD_IFCS | TX_CMD_RPS | TX_CMD_RS); 
        self.status.write(0);
    }

    fn wait_for_packet_tx(&self) {
        while (self.status.read() & TX_STATUS_DD) == 0 {
            // debug!("tx desc status: {}", self.status.read());
        } 
    }
}

impl fmt::Debug for LegacyTxDesc {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{{addr: {:#X}, length: {}, cso: {}, cmd: {}, status: {}, css: {}, special: {}}}",
                    self.phys_addr.read(), self.length.read(), self.cso.read(), self.cmd.read(), self.status.read(), self.css.read(), self.vlan.read())
    }
}



/// This struct is a Legacy Receive Descriptor. 
/// There is one instance of this struct per receive buffer. 
#[repr(C)]
pub struct LegacyRxDesc {
    /// The starting physical address of the receive buffer
    pub phys_addr:  Volatile<u64>,      
    /// Length of the receive buffer in bytes
    pub length:     ReadOnly<u16>,
    /// Checksum value of the packet after the IP header till the end 
    pub checksum:   ReadOnly<u16>,
    /// Status bits which tell if the descriptor has been used
    pub status:     Volatile<u8>,
    /// Receive errors
    pub errors:     ReadOnly<u8>,
    /// Vlan tags
    pub vlan:       ReadOnly<u16>,
}

impl RxDescriptor for LegacyRxDesc {
    fn init(&mut self, packet_buffer_address: PhysicalAddress) {
        self.phys_addr.write(packet_buffer_address.value() as u64);
        self.status.write(0);
    }

    fn set_packet_address(&mut self, packet_buffer_address: PhysicalAddress) {
        self.phys_addr.write(packet_buffer_address.value() as u64);
    }

    fn reset_status(&mut self) {
        self.status.write(0);
    }

    fn descriptor_done(&self) -> bool {
        (self.status.read() & RX_STATUS_DD) == RX_STATUS_DD
    }

    fn end_of_packet(&self) -> bool {
        (self.status.read() & RX_STATUS_EOP) == RX_STATUS_EOP        
    }

    fn length(&self) -> u64 {
        self.length.read() as u64
    }
}

use core::fmt;
impl fmt::Debug for LegacyRxDesc {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{{addr: {:#X}, length: {}, checksum: {}, status: {}, errors: {}, special: {}}}",
                    self.phys_addr.read(), self.length.read(), self.checksum.read(), self.status.read(), self.errors.read(), self.vlan.read())
    }
}


/// Advanced Receive Descriptor used in the ixgbe driver (section 7.1.6 of 82599 datasheet).
/// It has 2 modes: Read and Write Back. There is one receive descriptor per receive buffer that can be converted between these 2 modes.
/// Read contains the addresses that the driver writes.
/// Write Back contains information the hardware writes on receiving a packet.
#[repr(C)]
pub struct AdvancedRxDesc {
    /// Starting physcal address of the receive buffer for the packet
    pub packet_buffer_address:  Volatile<u64>,
    /// Starting physcal address of the receive buffer for the header.
    /// This field will only be used if header splitting is enabled    
    pub header_buffer_address:  Volatile<u64>,
}

impl RxDescriptor for AdvancedRxDesc {
    fn init (&mut self, packet_buffer_address: PhysicalAddress) {
        self.packet_buffer_address.write(packet_buffer_address.value() as u64);
        // set the header address to 0 because packet splitting is not supposed to be enabled in the 82599
        self.header_buffer_address.write(0);
    }

    fn set_packet_address(&mut self, packet_buffer_address: PhysicalAddress) {
        self.packet_buffer_address.write(packet_buffer_address.value() as u64);
    }

    fn reset_status(&mut self) {
        self.header_buffer_address.write(0);
    }

    fn descriptor_done(&self) -> bool{
        (self.get_ext_status() & RX_STATUS_DD as u64) == RX_STATUS_DD as u64
    }

    fn end_of_packet(&self) -> bool {
        (self.get_ext_status() & RX_STATUS_EOP as u64) == RX_STATUS_EOP as u64        
    }

    fn length(&self) -> u64 {
        self.get_pkt_len() as u64
    }
}

impl AdvancedRxDesc {

    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Returns the packet type that was used for the Receive Side Scaling hash function.
    pub fn get_rss_type(&self) -> u64{
        self.packet_buffer_address.read().get_bits(0..3) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Returns the packet type as identified by the hardware.
    pub fn get_packet_type(&self) -> u64{
        self.packet_buffer_address.read().get_bits(4..16) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Returns the number of Receive Side Coalesced packets that start in this descriptor.
    pub fn get_rsccnt(&self) -> u64{
        self.packet_buffer_address.read().get_bits(17..20) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Returns the size of the packet header in bytes.
    pub fn get_hdr_len(&self) -> u64{
        self.packet_buffer_address.read().get_bits(21..30) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor.
    /// When set to 1b, indicates that the hardware has found the length of the header.
    pub fn get_sph(&self) -> bool{
        self.packet_buffer_address.read().get_bit(31) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Returns the Receive Side Scaling hash.
    pub fn get_rss_hash(&self) -> u64{
        self.packet_buffer_address.read().get_bits(32..63) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Returns the Flow Director Filter ID if the packet matches a filter.
    pub fn get_fdf_id(&self) -> u64{
        self.packet_buffer_address.read().get_bits(32..63) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Status information indicates whether a descriptor has been used 
    /// and whether the buffer is the last one for a packet
    pub fn get_ext_status(&self) -> u64{
        self.header_buffer_address.read().get_bits(0..19) 
    }
    
    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Returns errors reported by hardware for different packet types
    pub fn get_ext_error(&self) -> u64{
        self.header_buffer_address.read().get_bits(20..31) 
    }
    
    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Returns the number of bytes posted to the packet buffer
    pub fn get_pkt_len(&self) -> u64{
        self.header_buffer_address.read().get_bits(32..47) 
    }
    
    /// Write Back mode function for the Advanced Receive Descriptor.
    /// If the vlan header is stripped from the packet, then the 16 bits of the VLAN tag are posted here
    pub fn get_vlan_tag(&self) -> u64{
        self.header_buffer_address.read().get_bits(48..63) 
    }    
}

impl fmt::Debug for AdvancedRxDesc {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{{Packet buffer address: {:#X}, Packet header address: {:#X}}}",
            self.packet_buffer_address.read(), self.header_buffer_address.read())
    }
}


// Transmit descriptor bits
/// Tx Command: End of Packet
pub const TX_CMD_EOP:                      u8 = (1 << 0);     
/// Tx Command: Insert FCS
pub const TX_CMD_IFCS:                     u8 = (1 << 1);     
/// Tx Command: Insert Checksum
pub const TX_CMD_IC:                       u8 = (1 << 2);     
/// Tx Command: Report Status
pub const TX_CMD_RS:                       u8 = (1 << 3);     
/// Tx Command: Report Packet Sent
pub const TX_CMD_RPS:                      u8 = (1 << 4);     
/// Tx Command: VLAN Packet Enable
pub const TX_CMD_VLE:                      u8 = (1 << 6);     
/// Tx Command: Interrupt Delay Enable
pub const TX_CMD_IDE:                      u8 = (1 << 7);     
/// Tx Status: descriptor Done
pub const TX_STATUS_DD:                    u8 = 1 << 0;

// Receive descriptor bits 
/// Rx Status: Descriptor Done
pub const RX_STATUS_DD:                    u8 = 1 << 0;
/// Rx Status: End of Packet
pub const RX_STATUS_EOP:                   u8 = 1 << 1;



/// A struct to store the Rx descriptor queues for a nic.
pub struct RxQueues<T: RxDescriptor>{
    /// A vec of all the Rx descriptor queues
    pub queue: Vec<MutexIrqSafe<RxQueueInfo<T>>>,
    /// The number of Rx descriptor queues
    pub num_queues: u8
}

/// A struct that holds all information for one receive queue.
/// There should be one such object per queue
pub struct RxQueueInfo<T: RxDescriptor> {
    /// The number of the queue, stored here for our convenience.
    /// It should match its index in the `queue` field of the RxQueues struct
    pub id: u8,
    /// Receive descriptors
    pub rx_descs: BoxRefMut<MappedPages, [T]>,
    /// Current receive descriptor index
    pub rx_cur: u16,
    /// The list of rx buffers, in which the index in the vector corresponds to the index in `rx_descs`.
    /// For example, `rx_bufs_in_use[2]` is the receive buffer that will be used when `rx_descs[2]` is the current rx descriptor (rx_cur = 2).
    pub rx_bufs_in_use: Vec<ReceiveBuffer>,
    /// The queue of received Ethernet frames, ready for consumption by a higher layer.
    /// Just like a regular FIFO queue, newly-received frames are pushed onto the back
    /// and frames are popped off of the front.
    /// Each frame is represented by a Vec<ReceiveBuffer>, because a single frame can span multiple receive buffers.
    /// TODO: improve this? probably not the best cleanest way to expose received frames to higher layers   
    pub received_frames: VecDeque<ReceivedFrame>,
    /// The cpu which this queue is mapped to. 
    /// This in itself doesn't guarantee anything, but we use this value when setting the cpu id for interrupts and DCA.
    pub cpu_id: u8,
    /// The address where the rdt register is located for this queue
    pub rdt_addr: VirtualAddress,
}

impl<T: RxDescriptor> RxQueueInfo<T> {
    /// Updates the queue tail descriptor in the rdt register
    pub fn update_rdt(&self, val: u32) {
        unsafe { write_volatile((self.rdt_addr.value()) as *mut u32, val) }
    }
}



/// A struct to store the Tx descriptor queues for a nic.
pub struct TxQueues<T: TxDescriptor> {
    /// A vec of all the Tx descriptor queues 
    pub queue: Vec<MutexIrqSafe<TxQueueInfo<T>>>,
    /// The number of Tx descriptor queues 
    pub num_queues: u8
}

/// A struct that holds all information for a transmit queue. 
/// There should be one such object per queue.
pub struct TxQueueInfo<T: TxDescriptor> {
    /// The number of the queue, stored here for our convenience.
    /// It should match its index in the `queue` field of the TxQueues struct
    pub id: u8,
    /// Transmit descriptors 
    pub tx_descs: BoxRefMut<MappedPages, [T]>,
    /// Current transmit descriptor index
    pub tx_cur: u16,
    /// The cpu which this queue is mapped to. 
    /// This in itself doesn't guarantee anything but we use this value when setting the cpu id for interrupts and DCA.
    pub cpu_id : u8
}




/// Trait which contains functions for the NIC initialization procedure
pub trait NicInit {

    /// Returns the base address for the memory mapped registers from Base Address Register 0.
    /// 
    /// # Arguments
    /// * `dev`: the pci device we need to find the base address for
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
    /// * `dev`: the pci device we need to find the memory size for
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
    /// * `dev`: the pci device 
    /// * `mem_base`: the starting physical address of the device's memory mapped registers
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
    /// * `mem_base`: the starting physical address of the region that need to be allocated
    /// * `mem_size_in_bytes`: the size of the region that needs to be allocated 
    fn mem_map(mem_base: PhysicalAddress, mem_size_in_bytes: usize) -> Result<MappedPages, &'static str> {
        // inform the frame allocator that the physical frames where memory area for the nic exists
        // is now off-limits and should not be touched
        {
            let nic_area = PhysicalMemoryArea::new(mem_base, mem_size_in_bytes as usize, 1, 0); 
            get_frame_allocator().ok_or("NicInit::mem_map(): Couldn't get FRAME ALLOCATOR")?.lock().add_area(nic_area, false)?;
        }

        // set up virtual pages and physical frames to be mapped
        let pages_nic = allocate_pages_by_bytes(mem_size_in_bytes).ok_or("NicInit::mem_map(): couldn't allocated virtual page!")?;
        let frames_nic = FrameRange::from_phys_addr(mem_base, mem_size_in_bytes);
        let flags = nic_mapping_flags();

        // debug!("NicInit: memory base: {:#X}, memory size: {}", mem_base, mem_size_in_bytes);

        let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("NicInit::mem_map(): KERNEL_MMI was not yet initialized!")?;
        let mut kernel_mmi = kernel_mmi_ref.lock();
        let mut fa = get_frame_allocator().ok_or("NicInit::mem_map(): couldn't get FRAME_ALLOCATOR")?.lock();
        let nic_mapped_page = kernel_mmi.page_table.map_allocated_pages_to(pages_nic, frames_nic, flags, fa.deref_mut())?;

        Ok(nic_mapped_page)
    }

    /// Initialize the receive buffer pool from where receive buffers are taken and returned
    /// 
    /// # Arguments
    /// * `num_rx_buffers`: the amount of buffers that are initially added to the pool 
    /// * `buffer_size`: the size of the receive buffers in bytes
    /// * `rx_buffer_pool: the buffer pool to initialize
    fn init_rx_buf_pool(num_rx_buffers: usize, buffer_size: u16, rx_buffer_pool: &'static mpmc::Queue<ReceiveBuffer>) -> Result<(), &'static str> {
        let length = buffer_size;
        for _i in 0..num_rx_buffers {
            let (mp, phys_addr) = create_contiguous_mapping(length as usize, nic_mapping_flags())?; 
            let rx_buf = ReceiveBuffer::new(mp, phys_addr, length, rx_buffer_pool);
            if rx_buffer_pool.push(rx_buf).is_err() {
                // if the queue is full, it returns an Err containing the object trying to be pushed
                error!("ixgbe::init_rx_buf_pool(): rx buffer pool is full, cannot add rx buffer {}!", _i);
                return Err("ixgbe rx buffer pool is full");
            };
        }

        Ok(())
    }

    /// Steps to create and initialize a receive descriptor queue
    /// 
    /// # Arguments
    /// * `num_desc`: number of descriptors in the queue
    /// * `rx_buffer_pool`: the pool from which to take receive buffers
    /// * `buffer_size`: the size of each buffer in the pool
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

        debug!("ixgbe::rx_init(): phys_addr of rx_desc: {:#X}", rx_descs_starting_phys_addr);
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

        debug!("ixgbe::tx_init(): phys_addr of tx_desc: {:#X}", tx_descs_starting_phys_addr);
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