#![no_std]

#![allow(dead_code)] //  to suppress warnings for unused functions/methods
#![allow(safe_packed_borrows)] // temporary, just to suppress unsafe packed borrows 
#![feature(rustc_private)]
#![feature(abi_x86_interrupt)]

#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
extern crate volatile;
extern crate alloc;
extern crate spin;
extern crate irq_safety;
extern crate kernel_config;
extern crate memory;
extern crate pci; 
extern crate owning_ref;
extern crate interrupts;
extern crate pic;
extern crate x86_64;
extern crate mpmc;
extern crate network_interface_card;
extern crate intel_ethernet;

pub mod test_e1000_driver;
mod regs;

use core::fmt;
use core::ops::DerefMut;
use spin::Once; 
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use irq_safety::MutexIrqSafe;
use volatile::{Volatile, ReadOnly};
use alloc::boxed::Box;
use memory::{get_kernel_mmi_ref, FRAME_ALLOCATOR, PhysicalAddress, FrameRange, EntryFlags, allocate_pages_by_bytes, MappedPages, PhysicalMemoryArea, create_contiguous_mapping};
use pci::{PciDevice, pci_read_32, pci_read_8, pci_write, pci_set_command_bus_master_bit};
use kernel_config::memory::PAGE_SIZE;
use owning_ref::BoxRefMut;
use interrupts::{eoi,register_interrupt};
use x86_64::structures::idt::{ExceptionStackFrame};
use network_interface_card::{NetworkInterfaceCard, TransmitBuffer, ReceiveBuffer, ReceivedFrame};
use intel_ethernet::{
    NicInit,
    descriptors::{LegacyTxDesc, LegacyRxDesc}
};

pub const INTEL_VEND:           u16 = 0x8086;  // Vendor ID for Intel 
pub const E1000_DEV:            u16 = 0x100E;  // Device ID for the e1000 Qemu, Bochs, and VirtualBox emmulated NICs
const PCI_BAR0:                 u16 = 0x10;
const PCI_INTERRUPT_LINE:       u16 = 0x3C;

const E1000_NUM_RX_DESC:        usize = 8;
const E1000_NUM_TX_DESC:        usize = 8;

/// Currently, each receive buffer is a single page.
const E1000_RX_BUFFER_SIZE_IN_BYTES:     u16 = PAGE_SIZE as u16;

/// Rx Status: End of Packet
pub const RX_EOP:               u8 =  1 << 1;
/// Rx Status: Descriptor done
pub const RX_DD:                u8 =  1 << 0;

/// Tx Status: Descriptor done
pub const TX_DD:                u8 =  1 << 0;

/// Interrupt type: Link Status Change
pub const INT_LSC:              u32 = 0x04;
/// Interrupt type: Receive Timer Interrupt
pub const INT_RX:               u32 = 0x80;


/// The single instance of the E1000 NIC.
/// TODO: in the future, we should support multiple NICs all stored elsewhere,
/// e.g., on the PCI bus or somewhere else.
static E1000_NIC: Once<MutexIrqSafe<E1000Nic>> = Once::new();

/// Returns a reference to the E1000Nic wrapped in a MutexIrqSafe,
/// if it exists and has been initialized.
pub fn get_e1000_nic() -> Option<&'static MutexIrqSafe<E1000Nic>> {
    E1000_NIC.try()
}

/// How many ReceiveBuffers are preallocated for this driver to use. 
const RX_BUFFER_POOL_SIZE: usize = 256; 
lazy_static! {
    /// The pool of pre-allocated receive buffers that are used by the E1000 NIC
    /// and temporarily given to higher layers in the networking stack.
    static ref RX_BUFFER_POOL: mpmc::Queue<ReceiveBuffer> = mpmc::Queue::with_capacity(RX_BUFFER_POOL_SIZE);
}

///struct to hold mapping of registers
#[repr(C)]
pub struct IntelE1000Registers {
    pub ctrl:                       Volatile<u32>,          // 0x0
    _padding0:                      [u8; 4],                // 0x4 - 0x7
    pub status:                     ReadOnly<u32>,          // 0x8
    _padding1:                      [u8; 180],              // 0xC - 0xBF
    
    /// Interrupt control registers
    pub icr:                        ReadOnly<u32>,          // 0xC0   
    _padding2:                      [u8; 12],               // 0xC4 - 0xCF
    pub ims:                        Volatile<u32>,          // 0xD0
    _padding3:                      [u8; 44],               // 0xD4 - 0xFF 

    /// Receive control register
    pub rctl:                       Volatile<u32>,          // 0x100
    _padding4:                      [u8; 764],              // 0x104 - 0x3FF

    /// Transmit control register
    pub tctl:                       Volatile<u32>,          // 0x400
    _padding5:                      [u8; 9212],             // 0x404 - 0x27FF

    /// The lower (least significant) 32 bits of the physical address of the array of receive descriptors.
    pub rdbal:                      Volatile<u32>,          // 0x2800
    /// The higher (most significant) 32 bits of the physical address of the array of receive descriptors.
    pub rdbah:                      Volatile<u32>,          // 0x2804
    /// The length in bytes of the array of receive descriptors.
    pub rdlen:                      Volatile<u32>,          // 0x2808
    _padding6:                      [u8; 4],                // 0x280C - 0x280F
    /// The receive descriptor head index, which points to the next available receive descriptor.
    pub rdh:                        Volatile<u32>,          // 0x2810
    _padding7:                      [u8; 4],                // 0x2814 - 0x2817
    /// The receive descriptor tail index, which points to the last available receive descriptor.
    pub rdt:                        Volatile<u32>,          // 0x2818  
    _padding8:                      [u8; 4068],             // 0x281C - 0x37FF

    /// The lower (least significant) 32 bits of the physical address of the array of transmit descriptors.
    pub tdbal:                      Volatile<u32>,          // 0x3800
    /// The higher (most significant) 32 bits of the physical address of the array of transmit descriptors.
    pub tdbah:                      Volatile<u32>,          // 0x3804
    /// The length in bytes of the array of transmit descriptors.
    pub tdlen:                      Volatile<u32>,          // 0x3808
    _padding9:                      [u8; 4],                // 0x380C - 0x380F
    /// The transmit descriptor head index, which points to the next available transmit descriptor.
    pub tdh:                        Volatile<u32>,          // 0x3810
    _padding10:                     [u8; 4],                // 0x3814 - 0x3817
    /// The transmit descriptor tail index, which points to the last available transmit descriptor.
    pub tdt:                        Volatile<u32>,          // 0x3818
    _padding11:                     [u8; 7140],             // 0x381C - 0x53FF
    
    /// The lower (least significant) 32 bits of the NIC's MAC hardware address.
    pub ral:                        Volatile<u32>,          // 0x5400
    /// The higher (most significant) 32 bits of the NIC's MAC hardware address.
    pub rah:                        Volatile<u32>,          // 0x5404
    _padding12:                     [u8; 109560],           // 0x5408 - 0x1FFFF END: 0x20000 (128 KB) ..116708
}



/// A struct representing an e1000 network interface card.
pub struct E1000Nic {
    /// Type of BAR0
    bar_type: u8,
    /// IO Base Address     
    io_base: u32,
    /// MMIO Base Address
    mem_base: PhysicalAddress,   
    /// A flag indicating if eeprom exists
    eeprom_exists: bool,
    ///interrupt number
    interrupt_num: u8,
    /// The actual MAC address burnt into the hardware of this E1000 NIC.
    mac_hardware: [u8; 6],
    /// The optional spoofed MAC address to use in place of `mac_hardware` when transmitting.  
    mac_spoofed: Option<[u8; 6]>,
    /// Receive Descriptors
    rx_descs: BoxRefMut<MappedPages, [LegacyRxDesc]>, 
    /// Transmit Descriptors 
    tx_descs: BoxRefMut<MappedPages, [LegacyTxDesc]>, 
    /// Current rx descriptor index
    rx_cur: u16,      
    /// Current tx descriptor index
    tx_cur: u16,
    /// The list of rx buffers, in which the index in the vector corresponds to the index in `rx_descs`.
    /// For example, `rx_bufs_in_use[2]` is the receive buffer that will be used when `rx_descs[2]` is the current rx descriptor (rx_cur = 2).
    rx_bufs_in_use: Vec<ReceiveBuffer>,
    /// memory-mapped control registers
    regs: BoxRefMut<MappedPages, IntelE1000Registers>,
    /// The queue of received Ethernet frames, ready for consumption by a higher layer.
    /// Just like a regular FIFO queue, newly-received frames are pushed onto the back
    /// and frames are popped off of the front.
    /// Each frame is represented by a Vec<ReceiveBuffer>, because a single frame can span multiple receive buffers.
    /// TODO: improve this? probably not the best cleanest way to expose received frames to higher layers
    received_frames: VecDeque<ReceivedFrame>,
}


impl NicInit for E1000Nic {}

impl NetworkInterfaceCard for E1000Nic {

    fn send_packet(&mut self, transmit_buffer: TransmitBuffer) -> Result<(), &'static str> {
        
        self.tx_descs[self.tx_cur as usize].phys_addr.write(transmit_buffer.phys_addr.value() as u64);
        self.tx_descs[self.tx_cur as usize].length.write(transmit_buffer.length);
        self.tx_descs[self.tx_cur as usize].cmd.write((regs::CMD_EOP | regs::CMD_IFCS | regs::CMD_RPS | regs::CMD_RS ) as u8);
        self.tx_descs[self.tx_cur as usize].status.write(0);

        let old_cur: u8 = self.tx_cur as u8;
        self.tx_cur = (self.tx_cur + 1) % (E1000_NUM_TX_DESC as u16);
        

        // debug!("THD {}", self.regs.tdh.read());
        // debug!("TDT!{}", self.regs.tdt.read());

        self.regs.tdt.write(self.tx_cur as u32);   
    
        // debug!("THD {}", self.regs.tdh.read());
        // debug!("TDT!{}", self.regs.tdt.read());
        // debug!("post-write, tx_descs[{}] = {:?}", old_cur, self.tx_descs[old_cur as usize]);
        // debug!("Value of tx descriptor address: {:x}", self.tx_descs[old_cur as usize].phys_addr);
        // debug!("Waiting for packet to send!");
        
        while (self.tx_descs[old_cur as usize].status.read() & TX_DD) == 0 {
            //debug!("THD {}",self.read_command(REG_TXDESCHEAD));
            //debug!("status register: {}",self.tx_descs[old_cur as usize].status);
        }
        //bit 0 should be set when done

        // debug!("Sent tx buffer [{}]: length: {}, paddr: {:#X}, vaddr: {:#X}", 
        //     old_cur, transmit_buffer.length, transmit_buffer.phys_addr, transmit_buffer.mp.start_address()
        // );  

        Ok(())
    }

    fn get_received_frame(&mut self) -> Option<ReceivedFrame> {
        self.received_frames.pop_front()
    }

    fn poll_receive(&mut self) -> Result<(), &'static str> {
        // check if the NIC has received a new frame that we need to handle
        if (self.rx_descs[self.rx_cur as usize].status.read() & RX_DD) != 0 {
            self.handle_receive()
        } else {
            // else, no new frames
            Ok(())
        }
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac_spoofed.unwrap_or(self.mac_hardware)
    }
}



/// functions that setup the NIC struct and handle the sending and receiving of packets
impl E1000Nic {
    /// Initializes the new E1000 network interface card that is connected as the given PciDevice.
    pub fn init(e1000_pci_dev: &PciDevice) -> Result<&'static MutexIrqSafe<E1000Nic>, &'static str> {
        use pic::PIC_MASTER_OFFSET;

        //debug!("e1000_nc bar_type: {0}, mem_base: {1}, io_base: {2}", e1000_nc.bar_type, e1000_nc.mem_base, e1000_nc.io_base);
        
        // Get interrupt number
        let interrupt_num = pci_read_8(e1000_pci_dev.bus, e1000_pci_dev.slot, e1000_pci_dev.func, PCI_INTERRUPT_LINE) + PIC_MASTER_OFFSET;
        debug!("e1000 IRQ number: {}", interrupt_num);

        // Type of BAR0
        let bar_type = (e1000_pci_dev.bars[0] as u8) & 0x01;    
        // IO Base Address
        let io_base = e1000_pci_dev.bars[0] & !1;     
        // memory mapped base address
        let mem_base = PhysicalAddress::new((e1000_pci_dev.bars[0] as usize) & !3)?; //hard coded for 32 bit, need to make conditional 

        let mut mapped_registers = Self::mem_map(e1000_pci_dev, mem_base)?;
        
        Self::start_link(&mut mapped_registers);
        
        let mac_addr_hardware = Self::read_mac_address_from_nic(&mut mapped_registers);
        //e1000_nc.clear_multicast();
        //e1000_nc.clear_statistics();
        
        Self::enable_interrupts(&mut mapped_registers);
        register_interrupt(interrupt_num, e1000_handler)?;

        let (rx_descs, rx_buffers) = Self::rx_init(&mut mapped_registers)?;
        let tx_descs = Self::tx_init(&mut mapped_registers)?;

        let e1000_nic = E1000Nic {
            bar_type: bar_type,
            io_base: io_base,
            mem_base: mem_base,
            eeprom_exists: false,
            interrupt_num: interrupt_num,
            mac_hardware: mac_addr_hardware,
            mac_spoofed: None,
            rx_descs: rx_descs,
            tx_descs: tx_descs,
            rx_cur: 0,
            tx_cur: 0,
            rx_bufs_in_use: rx_buffers,
            regs: mapped_registers,
            received_frames: VecDeque::new(),
        };
        
        let nic_ref = E1000_NIC.call_once(|| MutexIrqSafe::new(e1000_nic));
        Ok(nic_ref)
    }
    
    /// allocates memory for the NIC, starting address and size taken from the PCI BAR0
    fn mem_map(dev: &PciDevice, mem_base: PhysicalAddress) -> Result<BoxRefMut<MappedPages, IntelE1000Registers>, &'static str> {
        // set the bust mastering bit for this PciDevice, which allows it to use DMA
        pci_set_command_bus_master_bit(dev);

        // find out amount of space needed
        let mem_size_in_bytes = E1000Nic::determine_mem_size(dev) as usize;

        // inform the frame allocator that the physical frames where the PCI config space for the nic exists
        // is now off-limits and should not be touched
        {
            let nic_area = PhysicalMemoryArea::new(mem_base, mem_size_in_bytes as usize, 1, 0); // TODO: FIXME:  use proper acpi number 
            FRAME_ALLOCATOR.try().ok_or("e1000: Couldn't get FRAME ALLOCATOR")?.lock().add_area(nic_area, false)?;
        }

        // set up virtual pages and physical frames to be mapped
        let pages_nic = allocate_pages_by_bytes(mem_size_in_bytes).ok_or("e1000::mem_map(): couldn't allocated virtual page!")?;
        let frames_nic = FrameRange::from_phys_addr(mem_base, mem_size_in_bytes);
        let flags = EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE;

        let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("e1000:mem_map KERNEL_MMI was not yet initialized!")?;
        let mut kernel_mmi = kernel_mmi_ref.lock();
        let mut fa = FRAME_ALLOCATOR.try().ok_or("e1000::mem_map(): couldn't get FRAME_ALLOCATOR")?.lock();
        let nic_mapped_page = kernel_mmi.page_table.map_allocated_pages_to(pages_nic, frames_nic, flags, fa.deref_mut())?;
        let regs = BoxRefMut::new(Box::new(nic_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<IntelE1000Registers>(0))?;
            
        debug!("E1000 status register: {:#X}", regs.status.read());
        Ok(regs)
    }


    fn init_rx_buf_pool(num_rx_buffers: usize) -> Result<(), &'static str> {
        let length = E1000_RX_BUFFER_SIZE_IN_BYTES;
        for _i in 0..num_rx_buffers {
            let (mp, phys_addr) = create_contiguous_mapping(length as usize, Self::nic_mapping_flags())?; 
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
    fn read_mac_address_from_nic(regs: &mut IntelE1000Registers) -> [u8; 6] {
        let mac_32_low = regs.ral.read();
        let mac_32_high = regs.rah.read();

        let mut mac_addr = [0; 6]; 
        mac_addr[0] =  mac_32_low as u8;
        mac_addr[1] = (mac_32_low >> 8) as u8;
        mac_addr[2] = (mac_32_low >> 16) as u8;
        mac_addr[3] = (mac_32_low >> 24) as u8;
        mac_addr[4] =  mac_32_high as u8;
        mac_addr[5] = (mac_32_high >> 8) as u8;

        debug!("E1000: read hardware MAC address: {:02x?}", mac_addr);
        mac_addr
    }   

    /// Start up the network
    fn start_link(regs: &mut IntelE1000Registers) {
        let val = regs.ctrl.read();
        regs.ctrl.write(val | 0x40 | 0x20);

        let val = regs.ctrl.read();
        regs.ctrl.write(val & !(regs::CTRL_LRST) & !(regs::CTRL_ILOS) & !(regs::CTRL_VME) & !(regs::CTRL_PHY_RST));

        debug!("e1000::start_link(): REG_CTRL: {:#X}", regs.ctrl.read());
    }

    ///TODO: change to mapped pages, add reg to struct
    /// clear multicast registers
    /* pub fn clear_multicast (&self) {
        for i in 0..128{
        self.write_command(REG_MTA + (i * 4), 0);
        }
    }

    ///TODO: change to mapped pages, add reg to struct
    /// clear statistic registers
    pub fn clear_statistics (&self) {
        for i in 0..64{
        self.write_command(REG_CRCERRS + (i * 4), 0);
        }
    } */      

    /// Initialize the array of receive descriptors and their corresponding receive buffers,
    /// and returns a tuple including both of them.
    fn rx_init(regs: &mut IntelE1000Registers) -> Result<(BoxRefMut<MappedPages, [LegacyRxDesc]>, Vec<ReceiveBuffer>), &'static str> {
        Self::init_rx_buf_pool(RX_BUFFER_POOL_SIZE)?;

        let size_in_bytes_of_all_rx_descs = E1000_NUM_RX_DESC * core::mem::size_of::<LegacyRxDesc>();

        // Rx descriptors must be 16 byte-aligned, which is satisfied below because it's aligned to a page boundary.
        let (rx_descs_mapped_pages, rx_descs_starting_phys_addr) = create_contiguous_mapping(size_in_bytes_of_all_rx_descs, Self::nic_mapping_flags())?;

        // cast our physically-contiguous MappedPages into a slice of receive descriptors
        let mut rx_descs = BoxRefMut::new(Box::new(rx_descs_mapped_pages))
            .try_map_mut(|mp| mp.as_slice_mut::<LegacyRxDesc>(0, E1000_NUM_RX_DESC))?;

        // now that we've created the rx descriptors, we can fill them in with initial values
        let mut rx_bufs_in_use: Vec<ReceiveBuffer> = Vec::with_capacity(E1000_NUM_RX_DESC);
        for rd in rx_descs.iter_mut() {
            // obtain or create a receive buffer for each rx_desc
            let rx_buf = RX_BUFFER_POOL.pop()
                .ok_or("Couldn't obtain a ReceiveBuffer from the pool")
                .or_else(|_e| {
                    create_contiguous_mapping(E1000_RX_BUFFER_SIZE_IN_BYTES as usize, Self::nic_mapping_flags())
                        .map(|(buf_mapped, buf_paddr)| 
                            ReceiveBuffer::new(buf_mapped, buf_paddr, E1000_RX_BUFFER_SIZE_IN_BYTES, &RX_BUFFER_POOL)
                        )
                })?;
            let paddr = rx_buf.phys_addr;
            rx_bufs_in_use.push(rx_buf);
            rd.init(paddr);
        }
        
        debug!("e1000::rx_init(): phys_addr of rx_desc: {:#X}", rx_descs_starting_phys_addr);
        let rx_desc_phys_addr_lower  = rx_descs_starting_phys_addr.value() as u32;
        let rx_desc_phys_addr_higher = (rx_descs_starting_phys_addr.value() >> 32) as u32;
        
        // write the physical address of the rx descs array
        regs.rdbal.write(rx_desc_phys_addr_lower);
        regs.rdbah.write(rx_desc_phys_addr_higher);
        // write the length (in total bytes) of the rx descs array
        regs.rdlen.write(size_in_bytes_of_all_rx_descs as u32);
        
        // Write the head index (the first receive descriptor)
        regs.rdh.write(0);
        // Write the tail index.
        // Note that the e1000 SDM states that we should set the RDT (tail index) to the index *beyond* the last receive descriptor, 
        // so if you have 8 rx descs, you will set it to 8. 
        // However, this causes problems during the first burst of ethernet packets when you first enable interrupts, 
        // because the `rx_cur` counter won't be able to catch up with the head index properly. 
        // Thus, we set it to one less than that in order to prevent such bugs. 
        // This doesn't prevent all of the rx buffers from being used, they will still all be used fully.
        regs.rdt.write((E1000_NUM_RX_DESC - 1) as u32); 
        // TODO: document these various e1000 flags and why we're setting them
        regs.rctl.write(regs::RCTL_EN| regs::RCTL_SBP | regs::RCTL_LBM_NONE | regs::RTCL_RDMTS_HALF | regs::RCTL_BAM | regs::RCTL_SECRC  | regs::RCTL_BSIZE_2048);

        Ok((rx_descs, rx_bufs_in_use))
    }           
    
    /// Initialize the array of tramsmit descriptors and return them.
    fn tx_init(regs: &mut IntelE1000Registers) -> Result<BoxRefMut<MappedPages, [LegacyTxDesc]>, &'static str> {
        let size_in_bytes_of_all_tx_descs = E1000_NUM_TX_DESC * core::mem::size_of::<LegacyTxDesc>();

        // Tx descriptors must be 16 byte-aligned, which is satisfied below because it's aligned to a page boundary.
        let (tx_descs_mapped_pages, tx_descs_starting_phys_addr) = create_contiguous_mapping(size_in_bytes_of_all_tx_descs, Self::nic_mapping_flags())?;

        // cast our physically-contiguous MappedPages into a slice of transmit descriptors
        let mut tx_descs = BoxRefMut::new(Box::new(tx_descs_mapped_pages))
            .try_map_mut(|mp| mp.as_slice_mut::<LegacyTxDesc>(0, E1000_NUM_TX_DESC))?;

        // now that we've created the tx descriptors, we can fill them in with initial values
        for td in tx_descs.iter_mut() {
            td.init();
        }

        debug!("e1000::tx_init(): phys_addr of tx_desc: {:#X}", tx_descs_starting_phys_addr);
        let tx_desc_phys_addr_lower  = tx_descs_starting_phys_addr.value() as u32;
        let tx_desc_phys_addr_higher = (tx_descs_starting_phys_addr.value() >> 32) as u32;

        // write the physical address of the rx descs array
        regs.tdbal.write(tx_desc_phys_addr_lower);        
        regs.tdbah.write(tx_desc_phys_addr_higher);
        
        // write the length (in total bytes) of the rx descs array
        regs.tdlen.write(size_in_bytes_of_all_tx_descs as u32);
        
        // write the head index and the tail index (both 0 initially because there are no tx requests yet)
        regs.tdh.write(0);
        regs.tdt.write(0);
        regs.tctl.write(regs::TCTL_EN | regs::TCTL_PSP);

        Ok(tx_descs)
    }       
    
    /// Enable Interrupts 
    fn enable_interrupts(regs: &mut IntelE1000Registers) {
        //self.write_command(REG_IMASK ,0x1F6DC);
        //self.write_command(REG_IMASK ,0xff & !4);
    
        regs.ims.write(INT_LSC|INT_RX); //RXT and LSC
        regs.icr.read(); // clear all interrupts
    }      

    // reads status and clears interrupt
    fn clear_interrupt_status(&self) -> u32 {
        self.regs.icr.read()
    }

    /// Handle the receipt of an Ethernet frame. 
    /// This should be invoked whenever the NIC has a new received frame that is ready to be handled,
    /// either in a polling fashion or from a receive interrupt handler.
    fn handle_receive(&mut self) -> Result<(), &'static str> {
        let mut receive_buffers_in_frame: Vec<ReceiveBuffer> = Vec::new();
        let mut _total_packet_length: u16 = 0;

        // The main idea here is to go through all of the receive buffers that the NIC has populated,
        // and collect all of them into a single ethernet frame, i.e., a `ReceivedFrame`.
        // We go through each receive buffer and remove it from the list of buffers in use,
        // but then we need to replace that receive buffer with a new one that can be filled by the NIC
        // the next time it receives a piece of a frame. 
        
        // debug!("handle_receive(): rx_cur {}, head: {}, tail: {}\n\t[{:#X}, {:#X}, {:#X}, {:#X}, {:#X}, {:#X}, {:#X}, {:#X}]", 
        //     self.rx_cur, self.regs.rdh.read(), self.regs.rdt.read(),
        //     self.rx_descs[0].status.read(), 
        //     self.rx_descs[1].status.read(), 
        //     self.rx_descs[2].status.read(), 
        //     self.rx_descs[3].status.read(), 
        //     self.rx_descs[4].status.read(), 
        //     self.rx_descs[5].status.read(), 
        //     self.rx_descs[6].status.read(), 
        //     self.rx_descs[7].status.read(), 
        // );


        while (self.rx_descs[self.rx_cur as usize].status.read() & RX_DD) == RX_DD {
            // get information about the current receive buffer
            let length = self.rx_descs[self.rx_cur as usize].length.read();
            let status = self.rx_descs[self.rx_cur as usize].status.read();
            // debug!("e1000: received rx buffer [{}]: length: {}, status: {:#X}", self.rx_cur, length, status);

            // //print rx_buf
            // let length = self.rx_descs[self.rx_cur as usize].length;
            // let rx_buf = self.rx_bufs_in_use[self.rx_cur as usize].start_address() as *const u8;
            // //print rx_buf of length bytes
            // debug!("rx_buf {}: ", self.rx_cur);

            _total_packet_length += length;

            // Now that we are "removing" the current receive buffer from the list of receive buffers that the NIC can use,
            // (because we're saving it for higher layers to use),
            // we need to obtain a new `ReceiveBuffer` and set it up such that the NIC will use it for future receivals.
            let new_receive_buf = match RX_BUFFER_POOL.pop() {
                Some(rx_buf) => rx_buf,
                None => {
                    warn!("e1000 RX BUF POOL WAS EMPTY.... reallocating! This means that no task is consuming the accumulated received ethernet frames.");
                    // if the pool was empty, then we allocate a new receive buffer
                    let len = E1000_RX_BUFFER_SIZE_IN_BYTES;
                    let (mp, phys_addr) = create_contiguous_mapping(len as usize, Self::nic_mapping_flags())?;
                    ReceiveBuffer::new(mp, phys_addr, len, &RX_BUFFER_POOL)
                }
            };

            // actually tell the NIC about the new receive buffer, and that it's ready for use now
            self.rx_descs[self.rx_cur as usize].phys_addr.write(new_receive_buf.phys_addr.value() as u64);
            self.rx_descs[self.rx_cur as usize].status.write(0);

            // Swap in the new receive buffer at the index corresponding to this current rx_desc's receive buffer,
            // getting back the receive buffer that is part of the received ethernet frame
            self.rx_bufs_in_use.push(new_receive_buf);
            let mut current_rx_buf = self.rx_bufs_in_use.swap_remove(self.rx_cur as usize); 
            current_rx_buf.length = length; // set the ReceiveBuffer's length to the size of the actual packet received
            receive_buffers_in_frame.push(current_rx_buf);
            
            // move on to the next receive buffer to see if it's ready for us to take
            let old_cur = self.rx_cur as u32;
            self.rx_cur = (self.rx_cur + 1) % E1000_NUM_RX_DESC as u16;
            // debug!("writing rx buffer tail: old_cur {} -> rx_cur {}  (head: {}, tail: {})", old_cur, self.rx_cur, self.regs.rdh.read(), self.regs.rdt.read());
            self.regs.rdt.write(old_cur);

            // check if this rx buffer is the last piece of the frame (EOP)
            if (status & RX_EOP) == RX_EOP {
                // if so, then package it up as a received frame
                let buffers = core::mem::replace(&mut receive_buffers_in_frame, Vec::new());
                self.received_frames.push_back(ReceivedFrame(buffers));
            } else {
                warn!("e1000: Received multi-rxbuffer frame, this scenario not fully tested!");
            }
        }

        Ok(())
    }


    /// The main interrupt handling routine for the e1000 NIC.
    /// This should be invoked from the actual interrupt handler entry point.
    fn handle_interrupt(&mut self) -> Result<(), &'static str> {
        let status = self.clear_interrupt_status();        
        let mut handled = false;

        // a link status change
        if (status & INT_LSC) == INT_LSC {
            debug!("e1000::handle_interrupt(): link status changed");
            Self::start_link(&mut self.regs);
            handled = true;
        }

        // receiver timer interrupt
        if (status & INT_RX) == INT_RX {
            // debug!("e1000::handle_interrupt(): receive interrupt");
            self.handle_receive()?;
            handled = true;
        }

        if !handled {
            error!("e1000::handle_interrupt(): unhandled interrupt!  status: {:#X}", status);
        }
        //regs.icr.read(); //clear interrupt
        Ok(())
    }
}

extern "x86-interrupt" fn e1000_handler(_stack_frame: &mut ExceptionStackFrame) {
    if let Some(ref e1000_nic_ref) = E1000_NIC.try() {
        let mut e1000_nic = e1000_nic_ref.lock();
        if let Err(e) = e1000_nic.handle_interrupt() {
            error!("e1000_handler(): error handling interrupt: {:?}", e);
        }
        eoi(Some(e1000_nic.interrupt_num));
    } else {
        error!("BUG: e1000_handler(): E1000 NIC hasn't yet been initialized!");
    }

}
