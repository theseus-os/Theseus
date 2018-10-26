#![no_std]
#![feature(alloc)]

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

pub mod test_e1000_driver;
mod regs;

use core::ptr::{read_volatile, write_volatile};
use core::ops::DerefMut;
use spin::Once; 
use alloc::Vec;
use irq_safety::MutexIrqSafe;
use volatile::{Volatile, ReadOnly};
use alloc::boxed::Box;
use memory::{get_kernel_mmi_ref,FRAME_ALLOCATOR, MemoryManagementInfo, PhysicalAddress, Frame, PageTable, EntryFlags, FrameAllocator, allocate_pages, MappedPages,FrameIter, PhysicalMemoryArea, ActivePageTable};
use pci::{PciDevice, pci_read_32, pci_read_8, pci_write, pci_set_command_bus_master_bit};
use kernel_config::memory::PAGE_SIZE;
use owning_ref::BoxRefMut;
use interrupts::{eoi,register_interrupt};
use x86_64::structures::idt::{ExceptionStackFrame};

pub const INTEL_VEND:               u16 = 0x8086;  // Vendor ID for Intel 
pub const E1000_DEV:                u16 = 0x100E;  // Device ID for the e1000 Qemu, Bochs, and VirtualBox emmulated NICs
const PCI_BAR0:                 u16 = 0x10;
const PCI_INTERRUPT_LINE:       u16 = 0x3C;

const E1000_NUM_RX_DESC:        usize = 8;
const E1000_NUM_TX_DESC:        usize = 8;

const E1000_SIZE_RX_DESC:       usize = 16;
const E1000_SIZE_TX_DESC:       usize = 16;

const E1000_SIZE_RX_BUFFER:     usize = 2048;
const E1000_SIZE_TX_BUFFER:     usize = 256;

///Rx Status bits
pub const RX_EOP:               u8 = 1<<1; //End of Packet
pub const RX_DD:                u8 = 1<<0; //Descriptor Done

///Interrupts types
pub const INT_LSC:              u32 = 0x04; //Link Status Change
pub const INT_RX:               u32 = 0x80; //Receive Timer interrupt

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


///struct to hold mapping of registers
#[repr(C)]
pub struct IntelEthRegisters {
    pub ctrl:                       Volatile<u32>,          // 0x0
    _padding0:                      [u8;4],                 // 0x4 - 0x7
    pub status:                     ReadOnly<u32>,          // 0x8
    _padding1:                      [u8;180],               // 0xC - 0xBF
    
    //Interrupt registers
    pub icr:                        ReadOnly<u32>,          // 0xC0   
    _padding2:                      [u8;12],                // 0xC4 - 0xCF
    pub ims:                        Volatile<u32>,          // 0xD0
    _padding3:                      [u8;44],                // 0xD4 - 0xFF 

    //Receive control
    pub rctl:                       Volatile<u32>,          // 0x100
    _padding4:                      [u8;764],               // 0x104 - 0x3FF

    //Transmit control
    pub tctl:                       Volatile<u32>,          // 0x400
    _padding5:                      [u8;9212],              // 0x404 - 0x27FF

    //Receive    
    pub rdbal:                      Volatile<u32>,          // 0x2800
    pub rdbah:                      Volatile<u32>,          // 0x2804
    pub rdlen:                      Volatile<u32>,          // 0x2808
    _padding6:                      [u8;4],                 // 0x280C - 0x280F
    pub rdh:                        Volatile<u32>,          // 0x2810
    _padding7:                      [u8;4],                 // 0x2814 - 0x2817
    pub rdt:                        Volatile<u32>,          // 0x2818  
    _padding8:                      [u8;4068],              // 0x281C - 0x37FF

    //Transmit
    pub tdbal:                      Volatile<u32>,          // 0x3800
    pub tdbah:                      Volatile<u32>,          // 0x3804
    pub tdlen:                      Volatile<u32>,          // 0x3808
    _padding9:                      [u8;4],                 // 0x380C - 0x380F
    pub tdh:                        Volatile<u32>,          // 0x3810
    _padding10:                     [u8;4],                 // 0x3814 - 0x3817
    pub tdt:                        Volatile<u32>,          // 0x3818
    _padding11:                     [u8;7140],              // 0x381C - 0x53FF
    
    //Receive Address
    pub ral:                        Volatile<u32>,          // 0x5400
    pub rah:                        Volatile<u32>,          // 0x5404
    _padding12:                     [u8;109560],            // 0x5408 - 0x1FFFF END: 0x20000 (128 KB) ..116708
}



///trait for network functions
trait NetworkCard {
        fn send_packet(&mut self, p_data: usize, p_len: u16) -> Result<(), &'static str>;
        fn handle_receive(&mut self) -> Result<(), &'static str>;
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
        ///interrupt number
        interrupt_num: u8,
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
        /// registers
        regs: Option<BoxRefMut<MappedPages, IntelEthRegisters>>,
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


impl NetworkCard for Nic {
        
        /// Send a packet, called by a function higher in the network stack
        /// p_data is address of tranmit buffer, must be pointing to contiguous memory
        fn send_packet(&mut self, p_data: usize, p_len: u16) -> Result<(), &'static str> {
                
                //debug!("Value of tx descriptor address_translated: {:x}",ptr);
                let ptr = try!(translate_v2p(p_data));
                
                //debug!("Value of tx descriptor address_translated: {:x}",ptr);
                self.tx_descs[self.tx_cur as usize].addr = ptr as u64;
                self.tx_descs[self.tx_cur as usize].length = p_len;
                self.tx_descs[self.tx_cur as usize].cmd = (regs::CMD_EOP | regs::CMD_IFCS | regs::CMD_RPS | regs::CMD_RS ) as u8; //(1<<0)|(1<<1)|(1<<3)
                self.tx_descs[self.tx_cur as usize].status = 0;

                let old_cur: u8 = self.tx_cur as u8;
                self.tx_cur = (self.tx_cur + 1) % (E1000_NUM_TX_DESC as u16);
                

                if let Some(ref mut regs) = self.regs {
                        debug!("THD {}",regs.tdh.read());
                        debug!("TDT!{}",regs.tdt.read());

                        regs.tdt.write(self.tx_cur as u32);   
                
                        debug!("THD {}",regs.tdh.read());
                        debug!("TDT!{}",regs.tdt.read());
                        debug!("post-write, tx_descs[{}] = {:?}", old_cur, self.tx_descs[old_cur as usize]);
                        debug!("Value of tx descriptor address: {:x}",self.tx_descs[old_cur as usize].addr);
                        debug!("Waiting for packet to send!");
                }
                else {
                        error!("e1000: send_packet(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?");
                        return Err("e1000: send_packet(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?");
                }
                
                while (self.tx_descs[old_cur as usize].status & 0xF) == 0 {
                        //debug!("THD {}",self.read_command(REG_TXDESCHEAD));
                        //debug!("status register: {}",self.tx_descs[old_cur as usize].status);
                }  //bit 0 should be set when done

                debug!("Packet is sent!");  
                Ok(())
        }   

        /// Handle a packet reception.
        fn handle_receive(&mut self) -> Result<(), &'static str> {
                //print status of all packets until EoP
                while(self.rx_descs[self.rx_cur as usize].status & RX_DD) != 0 {
                        let status = self.rx_descs[self.rx_cur as usize].status;
                        debug!("Packet Received: rx desc status {}", status);
                        self.rx_descs[self.rx_cur as usize].status = 0;
                        let old_cur = self.rx_cur as u32;
                        self.rx_cur = (self.rx_cur + 1) % E1000_NUM_RX_DESC as u16;

                        if let Some(ref mut regs) = self.regs {
                                regs.rdt.write(old_cur);
                        }
                        else {
                                error!("e1000: check_state(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?");
                                return Err("e1000: check_state(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?");
                        }

                        //check if EOP
                        if (status & RX_EOP) == RX_EOP {
                                break;
                        }
                }

                // Print packets
                /* while (self.rx_descs[self.rx_cur as usize].status & 0xF) != 0{
                        //got_packet = true;
                        let length = self.rx_descs[self.rx_cur as usize].length;
                        let packet = self.rx_buf_addr[self.rx_cur as usize] as *const u8;
                        //print packet of length bytes
                        debug!("Packet {}: ", self.rx_cur);

                        for i in 0..length {
                                let points_at = unsafe{ *packet.offset(i as isize ) };
                                //debug!("{}",points_at);
                                debug!("{:x}",points_at);
                        }                    
                        

                        self.rx_descs[self.rx_cur as usize].status = 0;
                        let old_cur = self.rx_cur as u32;
                        self.rx_cur = (self.rx_cur + 1) % E1000_NUM_RX_DESC as u16;

                        self.write_command(REG_RXDESCTAIL, old_cur );
                } */
                
                Ok(())
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
        }

        ///allow pci device to be bus master
        pub fn set_master (&mut self,ref dev:&PciDevice) {
                 pci_set_command_bus_master_bit(dev.bus, dev.slot, dev.func);
        }

        ///find out amount of space needed for device's registers
        pub fn find_mem_size (&mut self,ref dev:&PciDevice) -> u32 {
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
        pub fn mem_map (&mut self,ref dev:&PciDevice) -> Result<(), &'static str>{
                self.set_master(dev);

                //find out amount of space needed
                let mem_size = self.find_mem_size(dev);

                // get a reference to the kernel's memory mapping information
                let kernel_mmi_ref = get_kernel_mmi_ref().expect("KERNEL_MMI was not yet initialized!");
                //let kernel_mmi_ref = try!(get_kernel_mmi_ref().ok_or("e1000:mem_map KERNEL_MMI was not yet initialized!"));
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

                //debug!("frames start: {:#X}, frames end {:#X}",frames_nic.start.start_address(),frames_nic.end.start_address());
                
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
                                let nic_mapped_page = try!(active_table.map_allocated_pages_to(pages_nic, frames_nic, mapping_flags, fa.deref_mut()));
                                //let result = active_table.map_allocated_pages_to(pages_nic, frames_nic, mapping_flags, fa.deref_mut());
                                
                                //NIC_PAGES.call_once(|| result);

                                let nic_regs = BoxRefMut::new(Box::new(nic_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<IntelEthRegisters>(0))?;

                                self.regs = Some(nic_regs);               
                                /* self.regs.call_once(|| {
                                        nic_regs
                                }); */
                        }
                        _ => { 
                                return Err("e1000:mem_map Couldn't get kernel's active_table");
                        }
                        

                } 
                //checking device status register
                self.check_dsr()?;
                Ok(())
        }

        ///check value of device status register
        pub fn check_dsr(&mut self) -> Result<(), &'static str> {
                let val: u32 = unsafe { read_volatile((self.mem_base + regs::REG_STATUS as usize) as *const u32) };
                debug!("original Status: {:#X}", val);

                if let Some(ref mut regs) = self.regs {
                        debug!("MappedPages status: {:#X}",regs.status.read());
                        Ok(())
                }
                else {
                        error!("e1000: check_dsr(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?");
                        Err("e1000: check_dsr(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?")
                }
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

        /// Read MAC Address
        pub fn read_mac_addr(&mut self) -> Result<(), &'static str> {

                if let Some(ref mut regs) = self.regs {
                        let mac_32_low = regs.ral.read();
                        let mac_32_high = regs.rah.read();

                        self.mac[0] = mac_32_low as u8;
                        self.mac[1] = (mac_32_low >> 8) as u8;
                        self.mac[2] = (mac_32_low >> 16) as u8;
                        self.mac[3] = (mac_32_low >> 24) as u8;
                        self.mac[4] = mac_32_high as u8;
                        self.mac[5] = (mac_32_high >> 8) as u8;

                        debug!("MAC address: {:#X?}",self.mac);

                        Ok(())
                }
                else {
                        error!("e1000: start_link(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?");
                        Err("e1000: start_link(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?")
                } 

        }   

        /// Start up the network
        pub fn start_link (&mut self) -> Result<(), &'static str> { 
                //for i217 just check that bit1 is set of reg status

                if let Some(ref mut regs) = self.regs {
                        let val = regs.ctrl.read();
                        regs.ctrl.write(val | 0x40 | 0x20);

                        let val = regs.ctrl.read();
                        regs.ctrl.write(val & !(regs::CTRL_LRST) & !(regs::CTRL_ILOS) & !(regs::CTRL_VME) & !(regs::CTRL_PHY_RST));

                        debug!("REG_CTRL: {:#X}", regs.ctrl.read());

                        Ok(())
                }
                else {
                        error!("e1000: start_link(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?");
                        Err("e1000: start_link(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?")
                }         
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

        /// Initialize receive descriptors and rx buffers
        pub fn rx_init(&mut self) -> Result<(), &'static str> {
                const NO_BYTES : usize = 16*(E1000_NUM_RX_DESC+1);
                
                let dma_ptr = self.nic_dma_allocator.allocate_dma_mem(NO_BYTES);
                let ptr;
                match dma_ptr{
                        Some(_x) => ptr = dma_ptr.unwrap(),
                        None => return Err("e1000:rx_init Couldn't allocate DMA mem for rx descriptors"),
                }

                let ptr1 = ptr + (16-(ptr%16));
                debug!("pointers: {:x}, {:x}",ptr, ptr1);

                let raw_ptr = ptr1 as *mut e1000_rx_desc;
                debug!("size of e1000_rx_desc: {}, e1000_tx_desc: {}", 
                        ::core::mem::size_of::<e1000_rx_desc>(), ::core::mem::size_of::<e1000_tx_desc>());
                
                unsafe{ self.rx_descs = Vec::from_raw_parts(raw_ptr, 0, E1000_NUM_RX_DESC);}
                //unsafe{debug!("Address of Rx desc: {:?}, value: {:?}",ptr, *pr1);}
                debug!("rx_descs: {:?}, capacity: {}", self.rx_descs, self.rx_descs.capacity());

                

                for i in 0..E1000_NUM_RX_DESC
                {
                        let dma_ptr = self.nic_dma_allocator.allocate_dma_mem(E1000_SIZE_RX_BUFFER);
                        match dma_ptr{
                                Some(_x) => self.rx_buf_addr[i] = dma_ptr.unwrap(),
                                None => return Err("e1000:rx_init Couldn't allocate DMA mem for rx buffer"),
                        } 

                        let buf_addr = try!(translate_v2p(self.rx_buf_addr[i]));
                        let mut var = e1000_rx_desc {
                                addr: buf_addr as u64,
                                length: 0,
                                checksum: 0,
                                status: 0,
                                errors: 0,
                                special: 0,
                        };
                                        
                        debug!("packet buffer: {:x}",var.addr);
                        self.rx_descs.push(var);
                
                }
                
                
                let slc = self.rx_descs.as_slice(); 
                let slc_ptr = slc.as_ptr();
                let v_addr = slc_ptr as usize;
                debug!("v address of rx_desc: {:x}",v_addr);

                let ptr = try!(translate_v2p(v_addr));
                
                debug!("p address of rx_desc: {:x}",ptr);
                let ptr1 = (ptr & 0xFFFF_FFFF) as u32;
                let ptr2 = (ptr>>32) as u32;

                
                if let Some(ref mut regs) = self.regs {
                        regs.rdbal.write(ptr1);//lowers bits of 64 bit descriptor base address, 16 byte aligned
                        regs.rdbah.write(ptr2);//upper 32 bits
                        
                        regs.rdlen.write((E1000_NUM_RX_DESC as u32)* 16);//number of bytes allocated for descriptors, 128 byte aligned
                        
                        regs.rdh.write(0);//head pointer for reeive descriptor buffer, points to 16B
                        regs.rdt.write(E1000_NUM_RX_DESC as u32);//Tail pointer for receive descriptor buffer, point to 16B
                        self.rx_cur = 0;
                        regs.rctl.write(regs::RCTL_EN| regs::RCTL_SBP | regs::RCTL_LBM_NONE | regs::RTCL_RDMTS_HALF | regs::RCTL_BAM | regs::RCTL_SECRC  | regs::RCTL_BSIZE_2048);
                
                        Ok(())
                }
                else {
                        error!("e1000: rx_init(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?");
                        Err("e1000: rx_init(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?")
                }

        }               
        
        /// Initialize transmit descriptors 
        pub fn tx_init(&mut self) -> Result<(), &'static str>  {
                const NO_BYTES: usize = 16*(E1000_NUM_TX_DESC+1);
                
                let dma_ptr = self.nic_dma_allocator.allocate_dma_mem(NO_BYTES);
                let ptr;
                match dma_ptr{
                        Some(_x) => ptr = dma_ptr.unwrap(),
                        None => return Err("e1000:tx_init Couldn't allocate DMA mem for tx descriptor"),
                } 

                // make sure memory is 16 byte aligned
                let ptr1 = ptr + (16-(ptr%16));
                debug!("tx pointers: {:x}, {:x}",ptr, ptr1);

                let raw_ptr = ptr1 as *mut e1000_tx_desc;
                unsafe{ self.tx_descs = Vec::from_raw_parts(raw_ptr, 0, E1000_NUM_TX_DESC);}
                //unsafe{debug!("Address of Rx desc: {:?}, value: {:?}",ptr, *pr1);}
               
                for _i in 0..E1000_NUM_TX_DESC
                {
                        let mut var = e1000_tx_desc {
                                addr: 0,
                                length: 0,
                                cso: 0,
                                cmd: 0,
                                status: 0,
                                css: 0,
                                special : 0,
                        };
                        self.tx_descs.push(var);
                }

                //TODO: don't need this, use ptr1
                let slc = self.tx_descs.as_slice(); 
                let slc_ptr = slc.as_ptr();
                let v_addr = slc_ptr as usize;
                debug!("v address of tx_desc: {:x}",v_addr);

                let ptr = try!(translate_v2p(v_addr));
                
                debug!("p address of tx_desc: {:x}",ptr);

                let ptr1 = (ptr & 0xFFFF_FFFF) as u32;
                let ptr2 = (ptr>>32) as u32;


                if let Some(ref mut regs) = self.regs {
                        regs.tdbah.write(ptr2);
                        regs.tdbal.write(ptr1);                
                        
                        //now setup total length of descriptors
                        regs.tdlen.write((E1000_NUM_TX_DESC as u32) * 16);                
                        
                        //setup numbers
                        regs.tdh.write(0);
                        regs.tdt.write(0);
                        self.tx_cur = 0;
                        regs.tctl.write(regs::TCTL_EN | regs::TCTL_PSP);
                        Ok(())
                }
                else {
                        error!("e1000: tx_init(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?");
                        Err("e1000: tx_init(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?")
                }                 
        }       
        
        /// Enable Interrupts 
        pub fn enable_interrupts(&mut self) -> Result<(), &'static str> {
                //self.write_command(REG_IMASK ,0x1F6DC);
                //self.write_command(REG_IMASK ,0xff & !4);
                
                if let Some(ref mut regs) = self.regs {
                        regs.ims.write(INT_LSC|INT_RX); //RXT and LSC
                        regs.icr.read(); // clear all interrupts
                        Ok(())
                }
                else {
                        error!("e1000: enable_interrupts(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?");
                        Err("e1000: enable_interrupts(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?")
                }
                
        }      

        pub fn check_state(&self) -> Result<(), &'static str> {

                if let Some(ref regs) = self.regs {
                        debug!("REG_CTRL {:x}", regs.ctrl.read());
                        debug!("REG_RCTRL {:x}", regs.rctl.read());
                        debug!("REG_TCTRL {:x}", regs.tctl.read());
                }
                else {
                        error!("e1000: check_state(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?");
                        return Err("e1000: check_state(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?");
                }
                

                debug!("addr {:x}",self.tx_descs[0].addr);// as *const u64);
                debug!("length {:?}",&self.tx_descs[0].length);// as *const u16);
                debug!("cso {:?}",&self.tx_descs[0].cso);// as *const u8);
                debug!("cmd {:?}",&self.tx_descs[0].cmd);// as *const u8);
                debug!("status {:?}",&self.tx_descs[0].status);// as *const u8);
                debug!("css {:?}",&self.tx_descs[0].css);// as *const u8);
                debug!("special {:?}",&self.tx_descs[0].special);// as *const u16);

                Ok(())
        }

        /// Poll for recieved messages
        /// Can be used as analternative to interrupts
        pub fn rx_poll(&mut self) -> Result<(), &'static str> {
                //detect if a packet has beem received
                if (self.rx_descs[self.rx_cur as usize].status&0xF) != 0 {                        
                        self.handle_receive()?
                }

                Ok(())
        }

        fn get_int_status(&mut self) -> Result<u32, &'static str> {
                if let Some(ref mut regs) = self.regs {
                        //reads status and clears interrupt
                        Ok(regs.icr.read())
                }
                else {
                        error!("e1000: handle_interrupt(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?");
                        Err("e1000: handle_interrupt(): FATAL ERROR: regs (IntelEthRegisters) were None! Were they initialized right?")
                }

        }

        //Interrupt handler for nic
        pub fn handle_interrupt(&mut self) -> Result<(), &'static str> {
                debug!("e1000 handler");
                let status = self.get_int_status()?;                

                if (status & INT_LSC ) == INT_LSC //link status change
                {
                        debug!("Interrupt:link status changed");
                        self.start_link()?;
                }
                else if (status & INT_RX) == INT_RX //receiver timer interrupt
                {
                        debug!("Interrupt: RXT");
                        self.handle_receive()?;
                }
                else{
                        debug!("Unhandled interrupt!");
                }
                //regs.icr.read(); //clear interrupt
                Ok(())
        }                               

}

/// static variable to represent the network card
lazy_static! {
        pub static ref E1000_NIC: MutexIrqSafe<Nic> = MutexIrqSafe::new(Nic{
                        bar_type : 0, 
                        io_base : 0,   
                        mem_base : 0, 
                        eeprom_exists: false,
                        interrupt_num: 0,
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
                                        },
                        regs: None,
                                        
                });
}

/// initialize the nic
pub fn init_nic(e1000_pci: &PciDevice) -> Result<(), &'static str>{
        use pic::PIC_MASTER_OFFSET;

        let mut e1000_nc = E1000_NIC.lock();       
        //debug!("e1000_nc bar_type: {0}, mem_base: {1}, io_base: {2}", e1000_nc.bar_type, e1000_nc.mem_base, e1000_nc.io_base);
        
        //Get interrupt number
        e1000_nc.interrupt_num = pci_read_8(e1000_pci.bus, e1000_pci.slot, e1000_pci.func, PCI_INTERRUPT_LINE) + PIC_MASTER_OFFSET;
        debug!("Int line: {}", e1000_nc.interrupt_num );

        e1000_nc.init(e1000_pci);
        e1000_nc.mem_map(e1000_pci)?;
        e1000_nc.mem_map_dma()?;
        
        e1000_nc.start_link()?;
        
        e1000_nc.read_mac_addr()?;
        //e1000_nc.clear_multicast();
        //e1000_nc.clear_statistics();
        
        e1000_nc.enable_interrupts()?;
        register_interrupt(e1000_nc.interrupt_num, e1000_handler);

        e1000_nc.rx_init()?;
        e1000_nc.tx_init()?;

       Ok(())
}

extern "x86-interrupt" fn e1000_handler(_stack_frame: &mut ExceptionStackFrame) {
    let mut nic = E1000_NIC.lock();
    let _ = nic.handle_interrupt();
    eoi(Some(nic.interrupt_num));
}

/// Poll for recieved messages
/// Can be used as analternative to interrupts
pub fn rx_poll(_: Option<u64>) -> Result<(), &'static str> {
        //debug!("e1000e poll function");
        loop {
                let mut e1000_nc = E1000_NIC.lock();
                //debug!("Rx status: {:#X}", e1000_nc.rx_descs[e1000_nc.rx_cur as usize].status);
                if (e1000_nc.rx_descs[e1000_nc.rx_cur as usize].status & RX_DD) != 0 {                        
                        e1000_nc.handle_receive()?;
                }
        }

        //Ok(())
        
}