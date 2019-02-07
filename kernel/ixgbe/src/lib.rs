#![no_std]
#![feature(alloc)]
#![feature(untagged_unions)]
#![allow(dead_code)] //  to suppress warnings for unused functions/methods
#![allow(safe_packed_borrows)] // temporary, just to suppress unsafe packed borrows 
#![feature(abi_x86_interrupt)]

#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
extern crate alloc;
extern crate spin;
extern crate irq_safety;
extern crate kernel_config;
extern crate memory;
extern crate pci; 
extern crate pit_clock;
extern crate bit_field;
extern crate interrupts;
extern crate x86_64;
extern crate apic;
extern crate pic;
extern crate acpi;
extern crate volatile;
extern crate mpmc;
extern crate network_interface_card;
extern crate owning_ref;

pub mod test_ixgbe_driver;
pub mod descriptors;
pub mod registers;

use core::ptr::{read_volatile, write_volatile};
use core::ops::DerefMut;
use spin::Once;
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use irq_safety::MutexIrqSafe;
use alloc::boxed::Box;
use memory::{get_kernel_mmi_ref,FRAME_ALLOCATOR, MemoryManagementInfo, PhysicalAddress, Frame, PageTable, EntryFlags, FrameAllocator, allocate_pages, MappedPages,FrameIter,PhysicalMemoryArea, allocate_pages_by_bytes, create_contiguous_mapping};
use pci::{get_pci_device_vd,PciDevice,pci_read_32, pci_read_8, pci_read_16, pci_write, pci_set_command_bus_master_bit,pci_set_interrupt_disable_bit, pci_enable_msi, PCI_BAR0, PCI_INTERRUPT_PIN};
use kernel_config::memory::PAGE_SIZE;
use descriptors::*;
use registers::*;
use bit_field::BitField;
use interrupts::{eoi,register_interrupt};
use x86_64::structures::idt::{ExceptionStackFrame};
use apic::get_my_apic_id;
use pic::PIC_MASTER_OFFSET;
use acpi::madt::redirect_interrupt;
use network_interface_card::{NetworkInterfaceCard, TransmitBuffer, ReceiveBuffer, ReceivedFrame};
use owning_ref::BoxRefMut;

//parameter that determine size of tx and rx descriptor queues
const IXGBE_NUM_RX_DESC:        usize = 8;
const IXGBE_NUM_TX_DESC:        usize = 8;

// const SIZE_RX_DESC:       usize = 16;
// const SIZE_TX_DESC:       usize = 16;

const IXGBE_RX_BUFFER_SIZE_IN_BYTES:     u16 = 8192;
const IXGBE_RX_HEADER_SIZE_IN_BYTES:     usize = 0;
const IXGBE_TX_BUFFER_SIZE_IN_BYTES:     usize = 256;

const IXGBE_NUM_RX_QUEUES:       usize = 1;

/// to hold memory mappings
static NIC_PAGES: Once<MappedPages> = Once::new();
static NIC_DMA_PAGES: Once<MappedPages> = Once::new();

/// Tx Status: Descriptor done
pub const TX_DD:                u8 = 1 << 0;
pub const RX_DD:                u8 = 1 << 0;
pub const RX_EOP:               u8 = 1 << 1;


/// The single instance of the 82599 NIC.
/// TODO: in the future, we should support multiple NICs all stored elsewhere,
/// e.g., on the PCI bus or somewhere else.
pub static IXGBE_NIC: Once<MutexIrqSafe<IxgbeNic>> = Once::new();

/// Returns a reference to the IxgbeNic wrapped in a MutexIrqSafe,
/// if it exists and has been initialized.
pub fn get_ixgbe_nic() -> Option<&'static MutexIrqSafe<IxgbeNic>> {
    IXGBE_NIC.try()
}

/// How many ReceiveBuffers are preallocated for this driver to use. 
const RX_BUFFER_POOL_SIZE: usize = 256; 
lazy_static! {
    /// The pool of pre-allocated receive buffers that are used by the E1000 NIC
    /// and temporarily given to higher layers in the networking stack.
    static ref RX_BUFFER_POOL: mpmc::Queue<ReceiveBuffer> = mpmc::Queue::with_capacity(RX_BUFFER_POOL_SIZE);
}

/// The mapping flags used for pages that the NIC will map.
/// This should be a const, but Rust doesn't yet allow constants for the bitflags type
/// TODO: This function is a repeat from the e1000, we need to consolidate common functions
fn nic_mapping_flags() -> EntryFlags {
    EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE
}

/// A struct representing an ixgbe netwrok interface card
pub struct IxgbeNic {
    /// Type of BAR0
    bar_type: u8,
    /// IO Base Address     
    io_base: u32,
    /// MMIO Base Address     
    mem_base: usize,   
    /// A flag indicating if eeprom exists
    eeprom_exists: bool,
    /// interrupt number
    interrupt_num: u8,
    /// The actual MAC address burnt into the hardware  
    mac_hardware: [u8;6],       
    /// The optional spoofed MAC address to use in place of `mac_hardware` when transmitting.  
    mac_spoofed: Option<[u8; 6]>,
    /// Receive Descriptors
    /// Should have one descriptor ring for each Rx queue
    rx_descs: Vec<BoxRefMut<MappedPages, [AdvancedReceiveDescriptorR]>>, 
    /// Transmit Descriptors 
    tx_descs: BoxRefMut<MappedPages, [E1000TxDesc]>, 
    /// Current Receive Descriptor index per queue
    rx_cur: [u16; IXGBE_NUM_RX_QUEUES],      
    /// Current Transmit Descriptor index
    tx_cur: u16,
    /// The list of rx buffers, in which the index in the vector corresponds to the index in `rx_descs`.
    /// For example, `rx_bufs_in_use[2]` is the receive buffer that will be used when `rx_descs[2]` is the current rx descriptor (rx_cur = 2).
    /// should have one list for each rx queue
    rx_bufs_in_use: Vec<Vec<ReceiveBuffer>>, 
    /// memory-mapped control registers
    regs: BoxRefMut<MappedPages, IntelIxgbeRegisters>,
    /// The queue of received Ethernet frames, ready for consumption by a higher layer.
    /// Just like a regular FIFO queue, newly-received frames are pushed onto the back
    /// and frames are popped off of the front.
    /// Each frame is represented by a Vec<ReceiveBuffer>, because a single frame can span multiple receive buffers.
    /// TODO: improve this? probably not the best cleanest way to expose received frames to higher layers
    /// TODO: Currently not using this for the IXGBE driver because no network stack. Will directly pass receive buffers to application.
    received_frames: VecDeque<ReceivedFrame>,
}

impl NetworkInterfaceCard for IxgbeNic {

    /// Right now packets are being sent assumming only one TX queue, which is why tx_queue[0] is being used
    fn send_packet(&mut self, transmit_buffer: TransmitBuffer) -> Result<(), &'static str>{
            
        self.tx_descs[self.tx_cur as usize].phys_addr.write(transmit_buffer.phys_addr as u64);
        self.tx_descs[self.tx_cur as usize].length.write(transmit_buffer.length);
        self.tx_descs[self.tx_cur as usize].cmd.write((CMD_EOP | CMD_IFCS | CMD_RPS | CMD_RS) as u8); 
        self.tx_descs[self.tx_cur as usize].status.write(0);

        let old_cur: u8 = self.tx_cur as u8;
        self.tx_cur = (self.tx_cur + 1) % (IXGBE_NUM_TX_DESC as u16);

        // debug!("TDH {}", self.regs.tx_regs.tx_queue[0].tdh.read());
        // debug!("TDT!{}", self.regs.tx_regs.tx_queue[0].tdt.read());

        self.regs.tx_regs.tx_queue[0].tdt.write(self.tx_cur as u32);   
        
        // debug!("TDH {}", self.regs.tx_regs.tx_queue[0].tdh.read());
        // debug!("TDT!{}", self.regs.tx_regs.tx_queue[0].tdt.read());
        // debug!("post-write, tx_descs[{}] = {:?}", old_cur, self.tx_descs[old_cur as usize]);
        // debug!("Value of tx descriptor address: {:x}",self.tx_descs[old_cur as usize].phys_addr.read());
        // debug!("Waiting for packet to send!");
        
        // Wait for descriptor done bit to be set which indicates that the packet has been sent
        while (self.tx_descs[old_cur as usize].status.read() & TX_DD) == 0 {
            // debug!("tx desc status: {}", self.tx_descs[old_cur as usize].status.read());
        }  //bit 0 should be set when done
        
        debug!("Packet is sent!");  
        Ok(())
    }

    /// TODO: Currently not in use bcause not using received_frames to store incoming packets. Do not use this function.
    fn get_received_frame(&mut self) -> Option<ReceivedFrame> {
        self.received_frames.pop_front()
    }

    /// TODO: Need to adjust function for multiple receive queues. Do no use this function.
    /// Use our own polling function for individual receive queues
    fn poll_receive(&mut self) -> Result<(), &'static str> {
       Ok(())
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac_spoofed.unwrap_or(self.mac_hardware)
    }
}

/// functions that setup the NIC struct and handle the sending and receiving of packets
impl IxgbeNic{

    /// store required values from the devices PCI config space
    pub fn init(ixgbe_pci_dev: &PciDevice) -> Result<&'static MutexIrqSafe<IxgbeNic>, &'static str> {

        //TODO: should get from ACPI tables, not a constant. The value in the Pci space is of no use.
        // Found this experimentally
        let interrupt_num = 0x10;

        // Type of BAR0
        let bar_type = (ixgbe_pci_dev.bars[0] as u8) & 0x01;    

        // IO Base Address
        let io_base = ixgbe_pci_dev.bars[0] & !1;   

        // memory mapped base address
        let mem_base = (ixgbe_pci_dev.bars[0] as usize) & !3; //TODO: hard coded for 32 bit, need to make conditional      

        let mut mapped_registers = Self::mem_map(ixgbe_pci_dev, mem_base)?;

        Self::start_link(&mut mapped_registers);

        let mac_addr_hardware = Self::read_mac_address_from_nic(&mut mapped_registers);

        //Enable Interrupts
        // pci_enable_msi(ixgbe_pci_dev)?;
        // pci_set_interrupt_disable_bit(ixgbe_pci_dev.bus, ixgbe_pci_dev.slot, ixgbe_pci_dev.func);
        // Self::enable_interrupts(&mut mapped_registers);
        // register_interrupt(interrupt_num + PIC_MASTER_OFFSET, ixgbe_handler);
        // redirect_interrupt(interrupt_num, 119);

        let (rx_descs, rx_buffers) = Self::rx_init(&mut mapped_registers)?;
        let tx_descs = Self::tx_init(&mut mapped_registers)?;

        let ixgbe_nic = IxgbeNic {
            bar_type: bar_type,
            io_base: io_base,
            mem_base: mem_base,
            eeprom_exists: false,
            interrupt_num: interrupt_num,
            mac_hardware: mac_addr_hardware,
            mac_spoofed: None,
            rx_descs: rx_descs,
            tx_descs: tx_descs,
            rx_cur: [0; IXGBE_NUM_RX_QUEUES],
            tx_cur: 0,
            rx_bufs_in_use: rx_buffers,
            regs: mapped_registers,
            received_frames: VecDeque::new(),
        };

        let nic_ref = IXGBE_NIC.call_once(|| MutexIrqSafe::new(ixgbe_nic));
        Ok(nic_ref)       
    }

    ///find out amount of space needed for device's registers
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

    /// allocates memory for the NIC, starting address and size taken from the PCI BAR0
    pub fn mem_map (dev: &PciDevice, mem_base: PhysicalAddress) -> Result<BoxRefMut<MappedPages, IntelIxgbeRegisters>, &'static str> {
        // set the bust mastering bit for this PciDevice, which allows it to use DMA
        pci_set_command_bus_master_bit(dev);

        //find out amount of space needed
        let mem_size_in_bytes = IxgbeNic::determine_mem_size(dev) as usize;

        // inform the frame allocator that the physical frames where the PCI config space for the nic exists
        // is now off-limits and should not be touched
        {
            let nic_area = PhysicalMemoryArea::new(mem_base, mem_size_in_bytes as usize, 1, 0); // TODO: FIXME:  use proper acpi number 
            FRAME_ALLOCATOR.try().ok_or("ixgbe: Couldn't get FRAME ALLOCATOR")?.lock().add_area(nic_area, false)?;
        }

        // set up virtual pages and physical frames to be mapped
        let pages_nic = allocate_pages_by_bytes(mem_size_in_bytes).ok_or("ixgbe::mem_map(): couldn't allocated virtual page!")?;
        let frames_nic = Frame::range_inclusive_addr(mem_base, mem_size_in_bytes);
        let flags = EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE;

        debug!("Ixgbe: memory base: {:#X}, memory size: {}", mem_base, mem_size_in_bytes);

        let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("ixgbe:mem_map KERNEL_MMI was not yet initialized!")?;
        let mut kernel_mmi = kernel_mmi_ref.lock();

        let regs = if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
            let mut fa = FRAME_ALLOCATOR.try().ok_or("ixgbe::mem_map(): couldn't get FRAME_ALLOCATOR")?.lock();
            let nic_mapped_page = active_table.map_allocated_pages_to(pages_nic, frames_nic, flags, fa.deref_mut())?;
            
            BoxRefMut::new(Box::new(nic_mapped_page))
                .try_map_mut(|mp| mp.as_type_mut::<IntelIxgbeRegisters>(0))?
        } else {
            return Err("ixgbe:mem_map Couldn't get kernel's active_table");
        };
            
        debug!("Ixgbe status register: {:#X}", regs.status.read());
        Ok(regs)

    }

    fn init_rx_buf_pool(num_rx_buffers: usize) -> Result<(), &'static str> {
        let length = IXGBE_RX_BUFFER_SIZE_IN_BYTES;
        for _i in 0..num_rx_buffers {
            let (mp, phys_addr) = create_contiguous_mapping(length as usize, nic_mapping_flags())?; 
            let rx_buf = ReceiveBuffer::new(mp, phys_addr, length, &RX_BUFFER_POOL);
            if RX_BUFFER_POOL.push(rx_buf).is_err() {
                // if the queue is full, it returns an Err containing the object trying to be pushed
                error!("e1000::init_rx_buf_pool(): rx buffer pool is full, cannot add rx buffer {}!", _i);
                return Err("e1000 rx buffer pool is full");
            };
        }

        Ok(())
    }

    pub fn spoof_mac(&mut self, spoofed_mac_addr: [u8; 6]) {
        self.mac_spoofed = Some(spoofed_mac_addr);
    }

    /// Reads the actual MAC address burned into the NIC hardware.
    fn read_mac_address_from_nic(regs: &mut IntelIxgbeRegisters) -> [u8; 6] {
        let mac_32_low = regs.ral.read();
        let mac_32_high = regs.rah.read();

        let mut mac_addr = [0; 6]; 
        mac_addr[0] =  mac_32_low as u8;
        mac_addr[1] = (mac_32_low >> 8) as u8;
        mac_addr[2] = (mac_32_low >> 16) as u8;
        mac_addr[3] = (mac_32_low >> 24) as u8;
        mac_addr[4] =  mac_32_high as u8;
        mac_addr[5] = (mac_32_high >> 8) as u8;

        debug!("Ixgbe: read hardware MAC address: {:02x?}", mac_addr);
        mac_addr
    }   

    /// software reset of NIC to get it running
    /// TODO: Replace all magic numbers
    fn start_link (regs: &mut IntelIxgbeRegisters) {
        //disable interrupts: write to EIMC registers, 1 in b30-b0, b31 is reserved
        regs.eimc.write(0x7FFFFFFF);

        // master disable algorithm (sec 5.2.5.3.2)
        // global reset = sw reset + link reset 
        let val = regs.ctrl.read();
        regs.ctrl.write(val|CTRL_RST|CTRL_LRST);

        //wait 10 ms
        let _ =pit_clock::pit_wait(10000);

        //flow control.. write 0 TO FCTTV, FCRTL, FCRTH, FCRTV and FCCFG to disable
        for i in 0..3 {
            regs.fcttv.reg[i].write(0);
        }

        for i in 0..7 {
            regs.fcrtl.reg[i].write(0);
            regs.fcrth.reg[i].write(0);
        }
        
        regs.fcrtv.write(0);
        regs.fccfg.write(0);

        //disable interrupts
        regs.eims.write(0x7FFFFFFF);

        //wait for eeprom auto read completion?

        //read MAC address
        debug!("MAC address low: {:#X}", regs.ral.read());
        debug!("MAC address high: {:#X}", regs.rah.read() & 0xFFFF);

        //wait for dma initialization done (RDRXCTL.DMAIDONE)
        // TODO: Replace with a while loop
        debug!("RDRXCTL: {:#X}", regs.rdrxctl.read()); //b3 should be 1

        //setup PHY and the link
        debug!("AUTOC: {:#X}", regs.autoc.read()); 

        let mut val = regs.autoc.read();
        val = val & !(0x0000_E000) & !(0x0000_0200);
        regs.autoc.write(val|AUTOC_10G_PMA_PMD_PAR|AUTOC_FLU);

        let mut val = regs.autoc2.read();
        val = val & !(0x0003_0000);
        regs.autoc2.write(val|1<<17);

        let val = regs.autoc.read();
        regs.autoc.write(val|AUTOC_RESTART_AN); 


        debug!("STATUS: {:#X}", regs.status.read()); 
        debug!("CTRL: {:#X}", regs.ctrl.read());
        debug!("LINKS: {:#X}", regs.links.read()); //b7 and b30 should be 1 for link up 
        debug!("AUTOC: {:#X}", regs.autoc.read()); 
    }

    /// Initialize the array of receive descriptors and their corresponding receive buffers,
    /// and returns a tuple including both of them for all rx queues in use.
    pub fn rx_init(regs: &mut IntelIxgbeRegisters) -> Result<(Vec<BoxRefMut<MappedPages, [AdvancedReceiveDescriptorR]>>, Vec<Vec<ReceiveBuffer>>), &'static str>  {
        Self::init_rx_buf_pool(RX_BUFFER_POOL_SIZE)?;

        let size_in_bytes_of_all_rx_descs_per_queue = IXGBE_NUM_RX_DESC * core::mem::size_of::<AdvancedReceiveDescriptorR>();

        let mut rx_descs_all_queues = Vec::new();
        let mut rx_bufs_in_use_all_queues = Vec::new();
        
        for queue in 0..IXGBE_NUM_RX_QUEUES {
            // Rx descriptors must be 128 byte-aligned, which is satisfied below because it's aligned to a page boundary.
            let (rx_descs_mapped_pages, rx_descs_starting_phys_addr) = create_contiguous_mapping(size_in_bytes_of_all_rx_descs_per_queue, nic_mapping_flags())?;

            // cast our physically-contiguous MappedPages into a slice of receive descriptors
            let mut rx_descs = BoxRefMut::new(Box::new(rx_descs_mapped_pages))
                .try_map_mut(|mp| mp.as_slice_mut::<AdvancedReceiveDescriptorR>(0, IXGBE_NUM_RX_DESC))?;

            // now that we've created the rx descriptors, we can fill them in with initial values
            let mut rx_bufs_in_use: Vec<ReceiveBuffer> = Vec::with_capacity(IXGBE_NUM_RX_DESC);
            for rd in rx_descs.iter_mut()
            {
                // obtain or create a receive buffer for each rx_desc
                let rx_buf = RX_BUFFER_POOL.pop()
                    .ok_or("Couldn't obtain a ReceiveBuffer from the pool")
                    .or_else(|_e| {
                        create_contiguous_mapping(IXGBE_RX_BUFFER_SIZE_IN_BYTES as usize, nic_mapping_flags())
                            .map(|(buf_mapped, buf_paddr)| 
                                ReceiveBuffer::new(buf_mapped, buf_paddr, IXGBE_RX_BUFFER_SIZE_IN_BYTES, &RX_BUFFER_POOL)
                            )
                    })?;
                let paddr = rx_buf.phys_addr;
                rx_bufs_in_use.push(rx_buf); 
                rd.init(paddr as u64, 0); // TODO: Currently setting header address to be 0 because using descriptors with out splitting. Need to check if it's supported because manual says it's not (Section 7.1.6.1)
            }

            debug!("ixgbe::rx_init(): phys_addr of rx_desc: {:#X}", rx_descs_starting_phys_addr);
            let rx_desc_phys_addr_lower  = rx_descs_starting_phys_addr as u32;
            let rx_desc_phys_addr_higher = (rx_descs_starting_phys_addr >> 32) as u32;

            // choose which set of rx queue registers needs to be accessed for this queue
            // because rx registers are divided into 2 sets of 64 queues in memory
            let (rx_queue_regs, queue_num);
            if queue < 64 {
                rx_queue_regs = &mut regs.rx_regs1;
                queue_num = queue;
            }
            else {
                rx_queue_regs = &mut regs.rx_regs2;
                queue_num = queue - 64;
            }
            
            // write the physical address of the rx descs ring
            rx_queue_regs.rx_queue[queue_num].rdbal.write(rx_desc_phys_addr_lower);
            rx_queue_regs.rx_queue[queue_num].rdbah.write(rx_desc_phys_addr_higher);
            // write the length (in total bytes) of the rx descs array
            rx_queue_regs.rx_queue[queue_num].rdlen.write(size_in_bytes_of_all_rx_descs_per_queue as u32); // should be 128 byte aligned, minimum 8 descriptors
            
            // Write the head index (the first receive descriptor)
            rx_queue_regs.rx_queue[queue_num].rdh.write(0);
            rx_queue_regs.rx_queue[queue_num].rdt.write(0);

            //set the size of the packet buffers and the descriptor format used
            let mut val = rx_queue_regs.rx_queue[queue_num].srrctl.read();
            val.set_bits(0..4,BSIZEPACKET_8K);
            val.set_bits(25..27,DESCTYPE_ADV_1BUFFER);
            rx_queue_regs.rx_queue[queue_num].srrctl.write(val);

            //enable the rx queue
            let mut val = rx_queue_regs.rx_queue[queue_num].rxdctl.read();
            val.set_bit(25, RX_Q_ENABLE);
            rx_queue_regs.rx_queue[queue_num].rxdctl.write(val);

            //make sure queue is enabled
            while rx_queue_regs.rx_queue[queue_num].rxdctl.read().get_bit(25) != RX_Q_ENABLE {}
            
            // Write the tail index.
            // Note that the 82599 datasheet (section 8.2.3.8.5) states that we should set the RDT (tail index) to the index *beyond* the last receive descriptor, 
            // so if you have 8 rx descs, you will set it to 8. 
            // However, this causes problems during the first burst of ethernet packets when you first enable interrupts, 
            // because the `rx_cur` counter won't be able to catch up with the head index properly. 
            // Thus, we set it to one less than that in order to prevent such bugs. 
            // This doesn't prevent all of the rx buffers from being used, they will still all be used fully.
            // same as what occurs in the e1000 descriptor
            rx_queue_regs.rx_queue[queue_num].rdt.write((IXGBE_NUM_RX_DESC - 1) as u32);

            rx_descs_all_queues.push(rx_descs);
            rx_bufs_in_use_all_queues.push(rx_bufs_in_use);
        
        }
        
        regs.fctrl.write(0x0000_0702); //TODO: remove magic number
        let val = regs.rxctrl.read();
        regs.rxctrl.write(val |1); //TODO: remove magic number

        Ok((rx_descs_all_queues, rx_bufs_in_use_all_queues))
    }

    //TODO: Remove magic numbers
    // pub fn set_filters(regs: &mut IntelIxgbeRegisters) -> Result<(), &'static str> {
    //     let val = regs.etqf.read();
    //     regs.etqf[0].write(val | 0x800 | 0x1000_0000);

    //     let val = regs.etself.read_command(REG_ETQF + 4);
    //     self.write_command(REG_ETQF + 4, val | 0x800 | 0x1000_0000);

    //     self.write_command(REG_ETQS, 0x8000_0000);
    //     self.write_command(REG_ETQS + 4, 0x8001_0000);

    //     Ok(())
    // }
    
    /// enable receive side scaling to allow packets to be received on multiple queues
    pub fn set_rss(regs: &mut IntelIxgbeRegisters) {
        let mut val = regs.mrqc.read();
        val.set_bits(0..3,RSS_ONLY);
        val.set_bits(16..31, RSS_UDPIPV4);
        regs.mrqc.write(val);
    }

    /// Initialize the array of tramsmit descriptors and return them.
    fn tx_init(regs: &mut IntelIxgbeRegisters) -> Result<BoxRefMut<MappedPages, [E1000TxDesc]>, &'static str>   {
        //disable transmission
        let val = regs.dmatxctl.read();
        regs.dmatxctl.write(val & !TE); 

        //let val = self.read_command(REG_RTTDCS);
        //self.write_command(REG_RTTDCS,val | 1<<6 ); // set b6 to 1

        let size_in_bytes_of_all_tx_descs = IXGBE_NUM_TX_DESC * core::mem::size_of::<E1000TxDesc>();
        
        // Tx descriptors must be 128 byte-aligned, which is satisfied below because it's aligned to a page boundary.
        let (tx_descs_mapped_pages, tx_descs_starting_phys_addr) = create_contiguous_mapping(size_in_bytes_of_all_tx_descs, nic_mapping_flags())?;

        // cast our physically-contiguous MappedPages into a slice of transmit descriptors
        let mut tx_descs = BoxRefMut::new(Box::new(tx_descs_mapped_pages))
            .try_map_mut(|mp| mp.as_slice_mut::<E1000TxDesc>(0, IXGBE_NUM_TX_DESC))?;

        // now that we've created the tx descriptors, we can fill them in with initial values
        for td in tx_descs.iter_mut() {
            td.init();
        }

        debug!("ixgbe::tx_init(): phys_addr of tx_desc: {:#X}", tx_descs_starting_phys_addr);
        let tx_desc_phys_addr_lower  = tx_descs_starting_phys_addr as u32;
        let tx_desc_phys_addr_higher = (tx_descs_starting_phys_addr >> 32) as u32;
        
        // TODO: Setting queue to 0 because we're only using one queue for transmission
        // Can change this to enable multiple tx queues
        let queue_num = 0;

        // write the physical address of the rx descs array
        regs.tx_regs.tx_queue[queue_num].tdbal.write(tx_desc_phys_addr_lower); 
        regs.tx_regs.tx_queue[queue_num].tdbah.write(tx_desc_phys_addr_higher); 

        // write the length (in total bytes) of the rx descs array
        regs.tx_regs.tx_queue[queue_num].tdlen.write(size_in_bytes_of_all_tx_descs as u32);               
        
        // write the head index and the tail index (both 0 initially because there are no tx requests yet)
        regs.tx_regs.tx_queue[queue_num].tdh.write(0);
        regs.tx_regs.tx_queue[queue_num].tdt.write(0);

        // enable transmit operation
        let val = regs.dmatxctl.read();
        regs.dmatxctl.write(val | TE); 

        //let val = self.read_command(REG_RTTDCS);
        //self.write_command(REG_RTTDCS,val & 0xFFFF_FFBF); // set b6 to 0
        
        //enable tx queue
        let mut val = regs.tx_regs.tx_queue[queue_num].txdctl.read();
        val.set_bit(25, TX_Q_ENABLE);
        regs.tx_regs.tx_queue[queue_num].txdctl.write(val); 

        //make sure queue is enabled
        while regs.tx_regs.tx_queue[queue_num].txdctl.read().get_bit(25) != TX_Q_ENABLE {} 

        Ok(tx_descs)
    }  

    /// enable interrupts
    /// TODO: change all magic numbers and update for multiple queues
    fn enable_interrupts(regs: &mut IntelIxgbeRegisters) {
        let queue_num = 0;
        //set IVAR reg for each queue used
        regs.ivar.reg[queue_num].write(0x81808180); // for rxq 0
        debug!("IVAR: {:#X}", regs.ivar.reg[queue_num].read());
        
        //enable clear on read of EICR
        let val = regs.gpie.read();
        regs.gpie.write((val & 0xFFFFFFDF) | 0x40); //bit 5
        //self.write_command(REG_GPIE, 0x46); //bit 5
        debug!("GPIE: {:#X}", regs.gpie.read());

        // self.write_command(REG_EIAM, 0xFFFF); // Rx0
        // debug!("EIAM: {:#X}", self.read_command(REG_EIAM));

        //set eims to enable required interrupt
        regs.eims.write(0xFFFF); // Rx 0
        debug!("EIMS: {:#X}", regs.eims.read());

        //self.write_command(REG_EITR, 0x8000_00C8); // Rx0
        debug!("EITR: {:#X}", regs.eitr.reg[queue_num].read());

        //clears eicr by writing 1 to clear old interrupt causes?
        let val = regs.eicr.read();
        debug!("EICR: {:#X}", val);
    }

    // reads status and clears interrupt
    fn clear_interrupt_status(&self) -> u32 {
        self.regs.eicr.read()
    }

    /* /// Handle a packet reception.
    pub fn handle_receive(&mut self) {
            //print status of all packets until EoP
            while(self.rx_descs[self.rx_cur as usize].status&0xF) !=0{
                    debug!("rx desc status {}",self.rx_descs[self.rx_cur as usize].status);
                    self.rx_descs[self.rx_cur as usize].status = 0;
                    let old_cur = self.rx_cur as u32;
                    self.rx_cur = (self.rx_cur + 1) % NUM_RX_DESC as u16;
                    self.write_command(REG_RDT, old_cur );
            }
    } */

    /// Returns all the receive buffers in one packet
    /// Called for individual queues
    /// Testing receive right now, not returning anything
    /// TODO: make more generic, right now works with our baseband processing application
    pub fn handle_receive(&mut self, queue_num: usize) -> Result<(), &'static str> {
        let mut receive_buffers_in_frame: Vec<ReceiveBuffer> = Vec::new();
        let mut total_packet_length: u16 = 0;
        let rx_descs = &mut self.rx_descs[queue_num];
        let rx_cur = self.rx_cur[queue_num] as usize;
        let rx_bufs_in_use = &mut self.rx_bufs_in_use[queue_num];

        // choose which set of rx queue registers needs to be accessed for this queue
        // because rx registers are divided into 2 sets of 64 queues in memory
        let (rx_queue_regs, queue);
        if queue_num < 64 {
            rx_queue_regs = &mut self.regs.rx_regs1;
            queue = queue_num;
        }
        else {
            rx_queue_regs = &mut self.regs.rx_regs2;
            queue = queue_num - 64;
        }

        //print status of all packets until EoP
        while(rx_descs[rx_cur].get_ext_status() & RX_DD as u64) == RX_DD as u64 {
            // get information about the current receive buffer
            let length = rx_descs[rx_cur].get_pkt_len();
            let status = rx_descs[rx_cur].get_ext_status();
            debug!("ixgbe: received rx buffer [{}]: length: {}, status: {:#X}", self.rx_cur[queue_num], length, status);

            total_packet_length += length as u16;

            // Now that we are "removing" the current receive buffer from the list of receive buffers that the NIC can use,
            // (because we're saving it for higher layers to use),
            // we need to obtain a new `ReceiveBuffer` and set it up such that the NIC will use it for future receivals.
            let new_receive_buf = match RX_BUFFER_POOL.pop() {
                Some(rx_buf) => rx_buf,
                None => {
                    warn!("IXGBE RX BUF POOL WAS EMPTY.... reallocating! This means that no task is consuming the accumulated received ethernet frames.");
                    // if the pool was empty, then we allocate a new receive buffer
                    let len = IXGBE_RX_BUFFER_SIZE_IN_BYTES;
                    let (mp, phys_addr) = create_contiguous_mapping(len as usize, nic_mapping_flags())?;
                    ReceiveBuffer::new(mp, phys_addr, len, &RX_BUFFER_POOL)
                }
            };

            // actually tell the NIC about the new receive buffer, and that it's ready for use now
            rx_descs[rx_cur].set_packet_buffer_address(new_receive_buf.phys_addr as u64);
            rx_descs[rx_cur].set_header_buffer_address(0);

            // Swap in the new receive buffer at the index corresponding to this current rx_desc's receive buffer,
            // getting back the receive buffer that is part of the received ethernet frame
            rx_bufs_in_use.push(new_receive_buf);
            let mut current_rx_buf = rx_bufs_in_use.swap_remove(rx_cur); 
            current_rx_buf.length = length as u16; // set the ReceiveBuffer's length to the size of the actual packet received
            receive_buffers_in_frame.push(current_rx_buf);

            // move on to the next receive buffer to see if it's ready for us to take
            let old_cur = rx_cur as u32;
            self.rx_cur[queue_num] = ((rx_cur + 1) % IXGBE_NUM_RX_DESC) as u16;
            rx_queue_regs.rx_queue[queue].rdt.write(old_cur); 

            if (status & RX_EOP as u64) == RX_EOP as u64 {
                break;
            }

        }

        Ok(())
    }

    /// The main interrupt handling routine for the ixgbe NIC.
    /// This should be invoked from the actual interrupt handler entry point.
    /// TODO: need to update, just a dummy handler
    fn handle_interrupt(&self) -> Result<(), &'static str> {
        let status = self.clear_interrupt_status();

        if (status & 0x01) == 0x01 { //Rx0
            debug!("Interrupt:packet received, status {:#X}", status);
        }
        else {
            debug!("Unhandled interrupt!, status {:#X}", status);
        }

        Ok(())
    }
}



/// initialize the nic
/// initalization process taken from section 4.6.3 of datasheet
pub fn init_nic(dev_pci: &PciDevice) -> Result<(), &'static str>{

    // let mut nic = NIC_82599.lock();       
    // //debug!("e1000_nc bar_type: {0}, mem_base: {1}, io_base: {2}", e1000_nc.bar_type, e1000_nc.mem_base, e1000_nc.io_base);
    
    // //TODO: should get from ACPI tables, not a constant
    // INTERRUPT_NO.call_once(|| 0x10 );

    // nic.init(dev_pci);
    // try!(nic.mem_map(dev_pci));

    // debug!("STATUS: {:#X}", nic.read_command(REG_STATUS));

    // try!(nic.mem_map_dma());
    
    // //disable interrupts: write to EIMC registers, 1 in b30-b0, b31 is reserved
    // nic.write_command(REG_EIMC, 0x7FFFFFFF);

    // // master disable algorithm (sec 5.2.5.3.2)
    // //global reset = sw reset + link reset 
    // let val = nic.read_command(REG_CTRL);
    // nic.write_command(REG_CTRL, val|CTRL_RST|CTRL_LRST);

    // //wait 10 ms
    // let _ =pit_clock::pit_wait(10000);

    // //flow control.. write 0 TO FCTTV, FCRTL, FCRTH, FCRTV and FCCFG to disable
    // for i in 0..3 {
    //         nic.write_command(REG_FCTTV + 4*i, 0);
    // }

    // for i in 0..7 {
    //         nic.write_command(REG_FCRTL + 4*i, 0);
    //         nic.write_command(REG_FCRTH + 4*i, 0);
    // }
    
    // nic.write_command(REG_FCRTV, 0);
    // nic.write_command(REG_FCCFG, 0);

    // //disable interrupts
    // nic.write_command(REG_EIMC, 0x7FFFFFFF);

    // //wait for eeprom auto read completion?

    // //read MAC address
    // debug!("MAC address low: {:#X}", nic.read_command(REG_RAL));
    // debug!("MAC address high: {:#X}", nic.read_command(REG_RAH) & 0xFFFF);

    // //wait for dma initialization done (RDRXCTL.DMAIDONE)

    // debug!("RDRXCTL: {:#X}",nic.read_command(REG_RDRXCTL)); //b3 should be 1

    // //setup PHY and the link
    // debug!("AUTOC: {:#X}", nic.read_command(REG_AUTOC)); 

    // let mut val = nic.read_command(REG_AUTOC);
    // val = val & !(0x0000_E000) & !(0x0000_0200);
    // nic.write_command(REG_AUTOC, val|AUTOC_10G_PMA_PMD_PAR|AUTOC_FLU);

    // let mut val = nic.read_command(REG_AUTOC2);
    // val = val & !(0x0003_0000);
    // nic.write_command(REG_AUTOC2, val|1<<17);

    // let val = nic.read_command(REG_AUTOC);
    // nic.write_command(REG_AUTOC, val|AUTOC_RESTART_AN); 


    // debug!("STATUS: {:#X}", nic.read_command(REG_STATUS)); 
    // debug!("CTRL: {:#X}", nic.read_command(REG_CTRL));
    // debug!("LINKS: {:#X}", nic.read_command(REG_LINKS)); //b7 and b30 should be 1 for link up 
    // debug!("AUTOC: {:#X}", nic.read_command(REG_AUTOC)); 

    //Initialize statistics

    //Enable Interrupts
    // pci_enable_msi(dev_pci)?;
    // pci_set_interrupt_disable_bit(dev_pci.bus, dev_pci.slot, dev_pci.func);
    // nic.enable_interrupts();
    // register_interrupt(*INTERRUPT_NO.try().unwrap() + PIC_MASTER_OFFSET, ixgbe_handler);
    // redirect_interrupt(*INTERRUPT_NO.try().unwrap(), 119);
    
    //Initialize transmit .. legacy descriptors

    // try!(nic.tx_init());
    // debug!("TXDCTL: {:#X}", nic.read_command(REG_TXDCTL)); //b25 should be 1 for tx queue enable

    // //initalize receive... legacy descriptors
    // try!(nic.rx_init_mq());

    //nic.set_filters();
    // nic.set_rss();

    Ok(())
}

// pub fn rx_poll(queue: usize){
//     //debug!("e1000e poll function");
//     loop {
//             let mut nic = NIC_82599.lock();

//     //detect if a packet has beem received
//     //debug!("E1000E_RX POLL");
//     /* for i in 0..NUM_RX_DESC {
//             debug!("rx desc status {}",self.rx_descs[i].status);
//     } */
//     //debug!("E1000E RCTL{:#X}",self.read_command(REG_RCTRL));
    
//             if (nic.rx_descs[nic.rx_cur as usize].status&0xF) != 0 {    
//                     debug!("Packet received!");                    
//                     nic.handle_receive();
//             }
//     }
    
// } 

pub fn rx_poll_mq(_: Option<u64>) -> Result<(), &'static str> {
    //debug!("e1000e poll function");
    loop {
        let mut nic = IXGBE_NIC.try().ok_or("e1000 NIC hasn't been initialized yet")?.lock();

        // for queue in 0..IXGBE_NUM_RX_QUEUES{
        //     let mut a = AdvancedReceiveDescriptorWB:: from(nic.rx_descs[queue][nic.rx_cur[queue] as usize]);
        //     if (a.get_ext_status()&0x1) != 0 {    
        //             debug!("Packet received in QUEUE{}!", queue);
        //             let _ = nic.handle_receive(queue);                    
        //     }       
        // }        
                
    }
    
}

extern "x86-interrupt" fn ixgbe_handler(_stack_frame: &mut ExceptionStackFrame) {
    if let Some(ref ixgbe_nic_ref) = IXGBE_NIC.try() {
        let mut ixgbe_nic = ixgbe_nic_ref.lock();
        if let Err(e) = ixgbe_nic.handle_interrupt() {
            error!("ixgbe_handler(): error handling interrupt: {:?}", e);
        }
        eoi(Some(ixgbe_nic.interrupt_num));
    } else {
        error!("BUG: ixgbe_handler(): IXGBE NIC hasn't yet been initialized!");
    }
}

// pub fn check_eicr(){
//     let nic = NIC_82599.lock();
//     debug!("EICR: {:#X}", nic.read_command(REG_EICR));
// }

// pub fn cause_interrupt(i_num :u32) {
    
//     let nic = NIC_82599.lock();
//     // for i in 0..16{
            
            
//     //         nic.write_command(REG_EIMS, i<<1);
//     //         nic.write_command(REG_EICS, i<<1);
            

//     //         //wait 10 ms
//     //         let _ =pit_clock::pit_wait(10000);
            
//     //         // if let Some(pci_dev_82599) = get_pci_device_vd(INTEL_VEND, INTEL_82599) {
//     //         //         debug!("status: {:#X}", pci_read_16(pci_dev_82599.bus, pci_dev_82599.slot, pci_dev_82599.func, PCI_STATUS));
//     //         // }


//     //         debug!("EICR: {:#X}", nic.read_command(REG_EICR));
//     // }

//     nic.write_command(REG_EIMS, i_num);
//     nic.write_command(REG_EICS, i_num);
    
// }

// pub fn ixgbe_handler() {
//         let nic = NIC_82599.lock();
//         nic.handle_interrupt();
// }

