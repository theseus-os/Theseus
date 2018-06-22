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

use core::ptr::{read_volatile, write_volatile};
use core::ops::DerefMut;
use spin::Once;
use alloc::Vec;
use irq_safety::MutexIrqSafe;
use alloc::boxed::Box;
use memory::{get_kernel_mmi_ref,FRAME_ALLOCATOR, MemoryManagementInfo, PhysicalAddress, Frame, PageTable, EntryFlags, FrameAllocator, allocate_pages, MappedPages,FrameIter,PhysicalMemoryArea};
use pci::{PciDevice,pci_read_32, pci_read_8, pci_write, get_pci_device_vd, pci_set_command_bus_master_bit};
use kernel_config::memory::PAGE_SIZE;

pub const INTEL_VEND:           u16 = 0x8086;  // Vendor ID for Intel 
pub const INTEL_82599:          u16 = 0x10FB;  // Device ID for the e1000 Qemu, Bochs, and VirtualBox emmulated NICs
const PCI_BAR0:                 u16 = 0x10;
const PCI_INTERRUPT_LINE:       u16 = 0x3C;

const REG_CTRL:                 u32 = 0x0000;
const REG_STATUS:               u32 = 0x0008;
const REG_EIMC:                 u32 = 0x0888;

const REG_FCTTV:                u32 = 0x3200; //+4*n with n=0..3
const REG_FCRTL:                u32 = 0x3220; //+4*n with n=0..7
const REG_FCRTH:                u32 = 0x3260; //+4*n with n=0..7
const REG_FCRTV:                u32 = 0x32A0;
const REG_FCCFG:                u32 = 0x3D00;

const REG_RDRXCTL:              u32 = 0x2F00;
const DMAIDONE:                 u32 = 1<<3;

const REG_RAL:                  u32 = 0xA200;
const REG_RAH:                  u32 = 0xA204;

const REG_AUTOC:                u32 = 0x42A0;
const REG_AUTOC2:               u32 = 0x42A8;
const REG_LINKS:                u32 = 0x42A4;

const AUTOC_FLU:                u32 = 1;
const AUTOC_LMS:                u32 = 4<<15; //KX/KX4//KR
const AUTOC_10G_PMA_PMD_PAR:    u32 = 1<<7;
const AUTOC2_10G_PMA_PMD_PAR:   u32 = 0<<8|0<<7; 
const AUTOC_RESTART_AN:         u32 = 1<<12;

const REG_EERD:                 u32 = 0x10014;

/******************/

const REG_EEPROM:               u32 = 0x0014;
const REG_CTRL_EXT:             u32 = 0x0018;
const REG_IMASK:                u32 = 0x00D0;
const REG_RCTRL:                u32 = 0x0100;
const REG_RXDESCLO:             u32 = 0x2800;
const REG_RXDESCHI:             u32 = 0x2804;
const REG_RXDESCLEN:            u32 = 0x2808;
const REG_RXDESCHEAD:           u32 = 0x2810;
const REG_RXDESCTAIL:           u32 = 0x2818;

const REG_TCTRL:                u32 = 0x0400;
const REG_TXDESCLO:             u32 = 0x3800;
const REG_TXDESCHI:             u32 = 0x3804;
const REG_TXDESCLEN:            u32 = 0x3808;
const REG_TXDESCHEAD:           u32 = 0x3810;
const REG_TXDESCTAIL:           u32 = 0x3818;

const REG_RDTR:                 u32 = 0x2820;    // RX Delay Timer Register
const REG_RXDCTL:               u32 = 0x3828;    // RX Descriptor Control
const REG_RADV:                 u32 = 0x282C;    // RX Int. Absolute Delay Timer
const REG_RSRPD:                u32 = 0x2C00;    // RX Small Packet Detect Interrupt
 
const REG_MTA:                  u32 = 0x5200; 
const REG_CRCERRS:              u32 = 0x4000;   
 
const REG_TIPG:                 u32 = 0x0410;      // Transmit Inter Packet Gap
const ECTRL_SLU:                u32 = 0x40;        // set link up

///CTRL commands
const CTRL_LRST:                u32 = (1<<3); 
const CTRL_RST:                 u32 = (1<<26);

/// RCTL commands
const RCTL_EN:                  u32 = (1 << 1);    // Receiver Enable
const RCTL_SBP:                 u32 = (1 << 2);    // Store Bad Packets
const RCTL_UPE:                 u32 = (1 << 3);    // Unicast Promiscuous Enabled
const RCTL_MPE:                 u32 = (1 << 4);    // Multicast Promiscuous Enabled
const RCTL_LPE:                 u32 = (1 << 5);    // Long Packet Reception Enable
const RCTL_LBM_NONE:            u32 = (0 << 6);    // No Loopback
const RCTL_LBM_PHY:             u32 = (3 << 6);    // PHY or external SerDesc loopback
const RTCL_RDMTS_HALF:          u32 = (0 << 8);    // Free Buffer Threshold is 1/2 of RDLEN
const RTCL_RDMTS_QUARTER:       u32 = (1 << 8);    // Free Buffer Threshold is 1/4 of RDLEN
const RTCL_RDMTS_EIGHTH:        u32 = (2 << 8);    // Free Buffer Threshold is 1/8 of RDLEN
const RCTL_MO_36:               u32 = (0 << 12);   // Multicast Offset - bits 47:36
const RCTL_MO_35:               u32 = (1 << 12);   // Multicast Offset - bits 46:35
const RCTL_MO_34:               u32 = (2 << 12);   // Multicast Offset - bits 45:34
const RCTL_MO_32:               u32 = (3 << 12);   // Multicast Offset - bits 43:32
const RCTL_BAM:                 u32 = (1 << 15);   // Broadcast Accept Mode
const RCTL_VFE:                 u32 = (1 << 18);   // VLAN Filter Enable
const RCTL_CFIEN:               u32 = (1 << 19);   // Canonical Form Indicator Enable
const RCTL_CFI:                 u32 = (1 << 20);   // Canonical Form Indicator Bit Value
const RCTL_DPF:                 u32 = (1 << 22);   // Discard Pause Frames
const RCTL_PMCF:                u32 = (1 << 23);   // Pass MAC Control Frames
const RCTL_SECRC:               u32 = (1 << 26);   // Strip Ethernet CRC
 
/// Buffer Sizes
const RCTL_BSIZE_256:           u32 = (3 << 16);
const RCTL_BSIZE_512:           u32 = (2 << 16);
const RCTL_BSIZE_1024:          u32 = (1 << 16);
const RCTL_BSIZE_2048:          u32 = (0 << 16);
const RCTL_BSIZE_4096:          u32 = ((3 << 16) | (1 << 25));
const RCTL_BSIZE_8192:          u32 = ((2 << 16) | (1 << 25));
const RCTL_BSIZE_16384:         u32 = ((1 << 16) | (1 << 25));
 
 
/// Transmit Command
 
const CMD_EOP:                  u32 = (1 << 0);    // End of Packet
const CMD_IFCS:                 u32 = (1 << 1);   // Insert FCS
const CMD_IC:                   u32 = (1 << 2);    // Insert Checksum
const CMD_RS:                   u32 = (1 << 3);   // Report Status
const CMD_RPS:                  u32 = (1 << 4);   // Report Packet Sent
const CMD_VLE:                  u32 = (1 << 6);    // VLAN Packet Enable
const CMD_IDE:                  u32 = (1 << 7);    // Interrupt Delay Enable
 
 
/// TCTL commands
 
const TCTL_EN:                  u32 = (1 << 1);    // Transmit Enable
const TCTL_PSP:                 u32 = (1 << 3);    // Pad Short Packets
const TCTL_CT_SHIFT:            u32 = 4;          // Collision Threshold
const TCTL_COLD_SHIFT:          u32 = 12;          // Collision Distance
const TCTL_SWXOFF:              u32 = (1 << 22);   // Software XOFF Transmission
const TCTL_RTLC:                u32 = (1 << 24);   // Re-transmit on Late Collision
 
const TSTA_DD:                  u32 = (1 << 0);    // Descriptor Done
const TSTA_EC:                  u32 = (1 << 1);    // Excess Collisions
const TSTA_LC:                  u32 = (1 << 2);    // Late Collision
const LSTA_TU:                  u32 = (1 << 3);    // Transmit Underrun

const E1000_NUM_RX_DESC:        usize = 8;
const E1000_NUM_TX_DESC:        usize = 8;

const E1000_SIZE_RX_DESC:       usize = 16;
const E1000_SIZE_TX_DESC:       usize = 16;

const E1000_SIZE_RX_BUFFER:     usize = 2048;
const E1000_SIZE_TX_BUFFER:     usize = 256;


/// to hold memory mappings
static NIC_PAGES: Once<MappedPages> = Once::new();
static NIC_DMA_PAGES: Once<MappedPages> = Once::new();

/// struct to represent receive descriptors
#[repr(C,packed)]
pub struct e1000_rx_desc {
        addr: u64,      
        length: u16,
        checksum: u16,
        status: u8,
        errors: u8,
        special: u16,
}
use core::fmt;
impl fmt::Debug for e1000_rx_desc {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "{{addr: {:#X}, length: {}, checksum: {}, status: {}, errors: {}, special: {}}}",
                        self.addr, self.length, self.checksum, self.status, self.errors, self.special)
        }
}

/// struct to represent transmission descriptors
#[repr(C,packed)]
pub struct e1000_tx_desc {
        addr: u64,
        length: u16,
        cso: u8,
        cmd: u8,
        status: u8,
        css: u8,
        special : u16,
}
impl fmt::Debug for e1000_tx_desc {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "{{addr: {:#X}, length: {}, cso: {}, cmd: {}, status: {}, css: {}, special: {}}}",
                        self.addr, self.length, self.cso, self.cmd, self.status, self.css, self.special)
        }
}

/// struct to hold information for the network card
pub struct Nic {
        /// Type of BAR0
        bar_type: u8,
        /// IO Base Address     
        io_base: u32,
        /// MMIO Base Address     
        mem_base: usize,   
        /// A flag indicating if eeprom exists
        eeprom_exists: bool,
        /// A buffer for storing the mac address  
        mac: [u8;6],       
        /// Receive Descriptors
        rx_descs: Vec<e1000_rx_desc>, 
        /// Transmit Descriptors 
        tx_descs: Vec<e1000_tx_desc>, 
        /// Current Receive Descriptor Buffer
        rx_cur: u16,      
        /// Current Transmit Descriptor Buffer
        tx_cur: u16,
        /// stores the virtual address of rx buffers
        rx_buf_addr: [usize;E1000_NUM_RX_DESC],
        /// The DMA allocator for the nic 
        nic_dma_allocator: DmaAllocator,
}

/// struct that stores addresses for memory allocated for DMA
pub struct DmaAllocator{
        /// starting address of physically contiguous memory
        start: usize,
        /// ending address of physically contiguous memory 
        end: usize, 
        /// starting address of available memory 
        current: usize, 
}

impl DmaAllocator{

        /// allocates DMA memory if amount is available, size is in bytes
        pub fn allocate_dma_mem(&mut self, size: usize) -> Option<usize> {
                let prev_current;                
                        
                if self.end-self.current > size {
                        prev_current = self.current;
                        self.current = self.current + size;
                        debug!("start: {:x} end: {:x} prev_current: {:x} current: {:x}", self.start,self.end,prev_current,self.current);
                        return Some(prev_current);
                }
                else{
                        return None;
                }

        }
}


/// translate virtual address to physical address
pub fn translate_v2p(v_addr : usize) -> Result<usize, &'static str> {
        
        // get a reference to the kernel's memory mapping information
        let kernel_mmi_ref = get_kernel_mmi_ref().expect("e1000: translate_v2p couldnt get ref to kernel mmi");
        let mut kernel_mmi_locked = kernel_mmi_ref.lock();
        // destructure the kernel's MMI so we can access its page table
        let MemoryManagementInfo { 
        page_table: ref mut kernel_page_table, 
        ..  // don't need to access other stuff in kernel_mmi
        } = *kernel_mmi_locked;
        match kernel_page_table {
                &mut PageTable::Active(ref mut active_table) => {
                        
                        let phys = try!(active_table.translate(v_addr).ok_or("e1000:translatev2p couldn't translate v addr"));
                        return Ok(phys); 
                        //return active_table.translate(v_addr);
                        
                }
                _ => { 
                        return Err("e1000:translatev2p kernel page table wasn't an ActivePageTable!"); 
                        //return None;
                }

        }

}



/// functions that setup the NIC struct and handle the sending and receiving of packets
impl Nic{

        /// store required values from the devices PCI config space
        pub fn init(&mut self,ref dev:&PciDevice){
                // Type of BAR0
                self.bar_type = (dev.bars[0] as u8) & 0x01;    
                // IO Base Address
                self.io_base = dev.bars[0] & !1;     
                // memory mapped base address
                self.mem_base = (dev.bars[0] as usize) & !3; //hard coded for 32 bit, need to make conditional      

                debug!("ixgbe::init {} {} {}", self.bar_type, self.io_base, self.mem_base);
                
        }

        /// allocates memory for the NIC, starting address and size taken from the PCI BAR0
        pub fn mem_map (&mut self,ref dev:&PciDevice) -> Result<(), &'static str>{
                pci_set_command_bus_master_bit(dev.bus, dev.slot, dev.func);
                //debug!("i217_mem_map: {0}, mem_base: {1}, io_basej: {2}", self.bar_type, self.mem_base, self.io_base);
                //debug!("usize bytes: {}", size_of(usize));

                //find out amount of space needed
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

                // get a reference to the kernel's memory mapping information
                //let kernel_mmi_ref = get_kernel_mmi_ref().expect("KERNEL_MMI was not yet initialized!");
                let kernel_mmi_ref = try!(get_kernel_mmi_ref().ok_or("e1000:mem_map KERNEL_MMI was not yet initialized!"));
                let mut kernel_mmi_locked = kernel_mmi_ref.lock();

                // destructure the kernel's MMI so we can access its page table
                let MemoryManagementInfo { 
                page_table: ref mut kernel_page_table, 
                ..  // don't need to access other stuff in kernel_mmi
                } = *kernel_mmi_locked;

                let no_pages: usize = mem_size as usize/PAGE_SIZE; //4K pages

                // inform the frame allocator that the physical frames where the PCI config space for the nic exists
                // is now off-limits and should not be touched
                {
                        let nic_area = PhysicalMemoryArea::new(self.mem_base as usize, mem_size as usize, 1, 0); // TODO: FIXME:  use proper acpi number 
                        try!(
                                try!(FRAME_ALLOCATOR.try().ok_or("e1000: Couldn't get FRAME ALLOCATOR")).lock().add_area(nic_area, false)
                        );
                }

                //allocate required no of pages
                //let pages_nic = allocate_pages(no_pages).expect("e1000::mem_map(): couldn't allocated virtual page!");
                let pages_nic = try!(allocate_pages(no_pages).ok_or("e1000::mem_map(): couldn't allocated virtual page!"));
                
                //allocate frames at address and create a FrameIter struct 
                let phys_addr = self.mem_base;
                let frame_first = Frame::containing_address(phys_addr as PhysicalAddress);
                
                let phys_addr = self.mem_base + (PAGE_SIZE*(no_pages-1)) as usize;
                let frame_last = Frame::containing_address(phys_addr as PhysicalAddress);

                let frames_nic = FrameIter {
                        start: frame_first,
                        end: frame_last,
                };

                debug!("frames start: {:#X}, frames end {:#X}",frames_nic.start.start_address(),frames_nic.end.start_address());
                
                let mapping_flags = EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE;
                

                // we can only map stuff if the kernel_page_table is Active
                // which you can be guaranteed it will be if you're in kernel code
                self.mem_base = pages_nic.pages.start.start_address();
                debug!("new mem_base: {:#X}",self.mem_base);
                
                match kernel_page_table {
                        &mut PageTable::Active(ref mut active_table) => {
                                let mut fa = try!(FRAME_ALLOCATOR.try().ok_or("e1000::mem_map(): couldn't get FRAME_ALLOCATOR")).lock();
                                //let mut fa = FRAME_ALLOCATOR.try().unwrap().lock();
                                // this maps just one page to one frame (4KB). If you need to map more, ask me
                                let result = try!(active_table.map_allocated_pages_to(pages_nic, frames_nic, mapping_flags, fa.deref_mut()));
                                //let result = active_table.map_allocated_pages_to(pages_nic, frames_nic, mapping_flags, fa.deref_mut());
                                
                                NIC_PAGES.call_once(|| result);
                        }
                        _ => { 
                                return Err("e1000:mem_map Couldn't get kernel's active_table");
                        }
                        

                }           
                
                Ok(())

        }

        /// allocates memory for DMA, will be used by the rx and tx descriptors
        pub fn mem_map_dma(&mut self) -> Result<(), &'static str> {
                
                // get a reference to the kernel's memory mapping information
                //let kernel_mmi_ref = get_kernel_mmi_ref().expect("KERNEL_MMI was not yet initialized!");
                let kernel_mmi_ref = try!(get_kernel_mmi_ref().ok_or("e1000:mem_map_dma KERNEL_MMI was not yet initialized!"));
                let mut kernel_mmi_locked = kernel_mmi_ref.lock();

                // destructure the kernel's MMI so we can access its page table
                let MemoryManagementInfo { 
                page_table: ref mut kernel_page_table, 
                ..  // don't need to access other stuff in kernel_mmi
                } = *kernel_mmi_locked;

                //let virt_addr = allocate_pages(1).expect("e1000::mem_map_dma(): couldn't allocated virtual page!");
                
                let num_pages;
                let num_frames;
                let bytes_required = (E1000_NUM_RX_DESC*E1000_SIZE_RX_BUFFER) + (E1000_NUM_RX_DESC * E1000_SIZE_RX_DESC) + E1000_SIZE_RX_DESC; //add an additional desc so we can make sure its 16-byte aligned
                if bytes_required % PAGE_SIZE == 0 {
                        num_pages =  bytes_required / PAGE_SIZE;
                        num_frames =  bytes_required / PAGE_SIZE;
                }
                else {
                       num_pages =  bytes_required / PAGE_SIZE + 1; 
                       num_frames =  bytes_required / PAGE_SIZE + 1;  
                }
                
                //let virt_addr = try!(allocate_pages(1).ok_or("e1000::mem_map_dma(): couldn't allocated virtual page!"));
                let virt_addr = try!(allocate_pages(num_pages).ok_or("e1000::mem_map_dma(): couldn't allocated virtual page!"));

                self.nic_dma_allocator.start = virt_addr.pages.start.start_address();
                self.nic_dma_allocator.current = virt_addr.pages.start.start_address();
                self.nic_dma_allocator.end = virt_addr.pages.end.start_address()+PAGE_SIZE; //end of dma memory
                trace!("head_pmem: {:#X}, tail_pmem: {:#X}", self.nic_dma_allocator.start, self.nic_dma_allocator.end);

                let mapping_flags = EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE;
                //debug!("page: {:?}, frame {:?}",page,frame);
                
                match kernel_page_table {
                &mut PageTable::Active(ref mut active_table) => {
                        let mut frame_allocator = try!(FRAME_ALLOCATOR.try().ok_or("e1000::mem_map_dma(): couldnt get FRAME_ALLOCATOR")).lock();
                        let frames = try!(frame_allocator.allocate_frames(num_frames).ok_or("e1000:mem_map_dma couldnt allocate a new frame"));
                        // let mut frame_allocator = FRAME_ALLOCATOR.try().unwrap().lock();
                        // let frame = frame_allocator.allocate_frame().unwrap() ;
                        
                        let result = try!(active_table.map_allocated_pages_to(virt_addr, frames, mapping_flags, frame_allocator.deref_mut()));
                        NIC_DMA_PAGES.call_once(|| result);
                }
                _ =>    { 
                                return Err("e1000:mem_map_dma Couldn't get kernel's active_table"); 
                        }

                }
                /**************************************/                
              
               Ok(())
        }

        /// write to an NIC register
        /// p_address is register offset
        fn write_command(&self,p_address:u32, p_value:u32){

                if self.bar_type == 0 
                {
                        unsafe { write_volatile((self.mem_base+p_address as usize) as *mut u32, p_value) };
                        //MMIOUtils::write32(mem_base+p_address,p_value);
                }
                //change port functions (later, not needed right now)
                else
                {
                        /*let IO_BASE_ADDRESS_PORT: Mutex<Port<u32>> = Mutex::new( Port::new(self.io_base));
                        let IO_BASE_DATA_PORT: Mutex<Port<u32>> = Mutex::new( Port::new(self.io_base+4));
                        IO_BASE_ADDRESS_PORT.lock().write(p_address); 
                        IO_BASE_DATA_PORT.lock().write(p_value); */
                }

        }

        /// read from an NIC register
        /// p_address is register offset
        fn read_command(&self,p_address:u32) -> u32 {
                let mut val: u32 = 0;
                if self.bar_type == 0 
                {
                        val = unsafe { read_volatile((self.mem_base+p_address as usize) as *const u32) };
                        //return MMIOUtils::read32(mem_base+p_address);
                }
                else
                {
                       /* let IO_BASE_ADDRESS_PORT: Mutex<Port<u32>> = Mutex::new( Port::new(self.io_base));
                        let IO_BASE_DATA_PORT: Mutex<Port<u32>> = Mutex::new( Port::new(self.io_base+4));
                        IO_BASE_ADDRESS_PORT.lock().write(p_address);
                        val = IO_BASE_DATA_PORT.lock().read(); 
                        //Ports::outportl(io_base, p_address);
                        //return Ports::inportl(io_base + 4);*/
                }
                val

        }

}

/// static variable to represent the network card
lazy_static! {
        pub static ref NIC_82599: MutexIrqSafe<Nic> = MutexIrqSafe::new(Nic{
                        bar_type : 0, 
                        io_base : 0,   
                        mem_base : 0, 
                        //mem_space : 0;
                        eeprom_exists: false,
                        mac: [0,0,0,0,0,0],
                        rx_descs: Vec::with_capacity(E1000_NUM_RX_DESC),
                        tx_descs: Vec::with_capacity(E1000_NUM_TX_DESC),
                        rx_cur: 0,
                        tx_cur: 0,
                        rx_buf_addr: [0;E1000_NUM_RX_DESC],
                        nic_dma_allocator: DmaAllocator{
                                                start: 0,
                                                end: 0,
                                                current: 0,
                                        }
                });
}

/// initialize the nic
/// initalization process taken from section 4.6.3 of datasheet
pub fn init_nic(dev_pci: &PciDevice) -> Result<(), &'static str>{

        let mut nic = NIC_82599.lock();       
        //debug!("e1000_nc bar_type: {0}, mem_base: {1}, io_base: {2}", e1000_nc.bar_type, e1000_nc.mem_base, e1000_nc.io_base);
        
        //pci_write(e1000_pci.bus, e1000_pci.slot, e1000_pci.func,PCI_INTERRUPT_LINE,0x2B);
        debug!("Int line: {}" ,pci_read_8(dev_pci.bus, dev_pci.slot, dev_pci.func, PCI_INTERRUPT_LINE));

        nic.init(dev_pci);
        try!(nic.mem_map(dev_pci));

        debug!("STATUS: {:#X}", nic.read_command(REG_STATUS));

        try!(nic.mem_map_dma());
        
        //disable interrupts: write to EIMC registers, 1 in b30-b0, b31 is reserved
        nic.write_command(REG_EIMC, 0x7FFFFFFF);

        // master disable algorithm (sec 5.2.5.3.2)
        //global reset = sw reset + link reset 
        let val = nic.read_command(REG_CTRL);
        nic.write_command(REG_CTRL, val|CTRL_RST|CTRL_LRST);

        //wait 10 ms
        let _ =pit_clock::pit_wait(10000);

        //flow control.. write 0 TO FCTTV, FCRTL, FCRTH, FCRTV and FCCFG to disable
        for i in 0..3 {
                nic.write_command(REG_FCTTV + 4*i, 0);
        }

        for i in 0..7 {
                nic.write_command(REG_FCRTL + 4*i, 0);
                nic.write_command(REG_FCRTH + 4*i, 0);
        }
        
        nic.write_command(REG_FCRTV, 0);
        nic.write_command(REG_FCCFG, 0);

        //disable interrupts
        nic.write_command(REG_EIMC, 0x7FFFFFFF);

        //wait for eeprom auto read completion?

        //read MAC address
        debug!("MAC address low: {:#X}", nic.read_command(REG_RAL));
        debug!("MAC address high: {:#X}", nic.read_command(REG_RAH) & 0xFFFF);

        //wait for dma initialization done (RDRXCTL.DMAIDONE)

        debug!("RDRXCTL: {:#X}",nic.read_command(REG_RDRXCTL)); //b3 should be 1

        //setup PHY and the link
        let val = nic.read_command(REG_AUTOC);
        nic.write_command(REG_AUTOC, val|AUTOC_LMS|AUTOC_10G_PMA_PMD_PAR|AUTOC_FLU);

        let val = nic.read_command(REG_AUTOC2);
        nic.write_command(REG_AUTOC2, val|AUTOC2_10G_PMA_PMD_PAR);

        let val = nic.read_command(REG_AUTOC);
        nic.write_command(REG_AUTOC, val|AUTOC_RESTART_AN);



        debug!("STATUS: {:#X}", nic.read_command(REG_STATUS)); 
        debug!("CTRL: {:#X}", nic.read_command(REG_CTRL));
        debug!("LINKS: {:#X}", nic.read_command(REG_LINKS)); //b7 and b30 should be 1 for link up 

        /*
        e1000_nc.detect_eeprom();
        e1000_nc.read_mac_addr();
        e1000_nc.start_link();
        e1000_nc.clear_multicast();
        e1000_nc.clear_statistics();
        try!(e1000_nc.rx_init());
        try!(e1000_nc.tx_init());
        e1000_nc.enable_interrupts(); */
        //e1000_nc.rx_init().unwrap();
        //e1000_nc.tx_init().unwrap();

       Ok(())
}
