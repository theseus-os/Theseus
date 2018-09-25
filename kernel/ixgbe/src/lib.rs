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
// extern crate interrupts;
extern crate x86_64;

pub mod test_tx;
pub mod descriptors;
pub mod registers;

use core::ptr::{read_volatile, write_volatile};
use core::ops::DerefMut;
use spin::Once;
use alloc::Vec;
use irq_safety::MutexIrqSafe;
use alloc::boxed::Box;
use memory::{get_kernel_mmi_ref,FRAME_ALLOCATOR, MemoryManagementInfo, PhysicalAddress, Frame, PageTable, EntryFlags, FrameAllocator, allocate_pages, MappedPages,FrameIter,PhysicalMemoryArea};
use pci::{PciDevice,pci_read_32, pci_read_8, pci_read_16, pci_write, pci_set_command_bus_master_bit, PCI_INTERRUPT_PIN, PCI_INTERRUPT_LINE, PCI_BAR0, PCI_CAPABILITIES, PCI_STATUS, MSI_CAPABILITY};
use kernel_config::memory::PAGE_SIZE;
use descriptors::*;
use registers::*;
use bit_field::BitField;
// use interrupts::{eoi,register_interrupt};
use x86_64::structures::idt::{ExceptionStackFrame};

//parameter that determine size of tx and rx descriptor queues
const NUM_RX_DESC:        usize = 8;
const NUM_TX_DESC:        usize = 8;

const SIZE_RX_DESC:       usize = 16;
const SIZE_TX_DESC:       usize = 16;

const SIZE_RX_BUFFER:     usize = 4096;
const SIZE_RX_HEADER:     usize = 256;
const SIZE_TX_BUFFER:     usize = 256;

const NO_RX_QUEUES:       usize = 16;

/// to hold memory mappings
static NIC_PAGES: Once<MappedPages> = Once::new();
static NIC_DMA_PAGES: Once<MappedPages> = Once::new();

///to hold interrupt number
static INTERRUPT_NO: Once<u8> = Once::new();

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
        rx_descs: [Vec<AdvancedReceiveDescriptorR>; NO_RX_QUEUES], 
        /// Transmit Descriptors 
        tx_descs: Vec<e1000_tx_desc>, 
        /// Current Receive Descriptor Buffer
        rx_cur: [u16; NO_RX_QUEUES],      
        /// Current Transmit Descriptor Buffer
        tx_cur: u16,
        /// stores the virtual address of rx buffers
        rx_buf_addr: [[usize;NUM_RX_DESC];NO_RX_QUEUES],
        /// The DMA allocator for the nic 
        nic_dma_allocator: DmaAllocator,
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
                let bytes_required = (NUM_RX_DESC * SIZE_RX_BUFFER * NO_RX_QUEUES) + (NUM_RX_DESC * SIZE_RX_HEADER * NO_RX_QUEUES) + (NUM_RX_DESC * SIZE_RX_DESC * NO_RX_QUEUES) + (NUM_TX_DESC * SIZE_TX_DESC) + PAGE_SIZE; //to make sure its 128 byte aligned
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

        /// Initialize receive descriptors and rx buffers for multiple queues
        pub fn rx_init_mq(&mut self) -> Result<(), &'static str> {

                for queue in 0..NO_RX_QUEUES {
                        const NO_BYTES : usize = 16*(NUM_RX_DESC) + 128;
                        let dma_ptr = self.nic_dma_allocator.allocate_dma_mem(NO_BYTES);
                        let ptr;
                        match dma_ptr{
                                Some(_x) => ptr = dma_ptr.unwrap(),
                                None => return Err("e1000:rx_init Couldn't allocate DMA mem for rx descriptors"),
                        }

                        let ptr1 = ptr + (128-(ptr%128));
                        debug!("pointers: {:x}, {:x}",ptr, ptr1);

                        let raw_ptr = ptr1 as *mut AdvancedReceiveDescriptorR;
                        debug!("size of e1000_rx_desc: {}, e1000_tx_desc: {}", 
                                ::core::mem::size_of::<e1000_rx_desc>(), ::core::mem::size_of::<e1000_tx_desc>());

                        unsafe{ self.rx_descs[queue] = Vec::from_raw_parts(raw_ptr, 0, NUM_RX_DESC);}
                         //unsafe{debug!("Address of Rx desc: {:?}, value: {:?}",ptr, *pr1);}
                        debug!("rx_descs: {:?}, capacity: {}", self.rx_descs, self.rx_descs[queue].capacity());

                        for i in 0..NUM_RX_DESC
                        {
                                let dma_ptr = self.nic_dma_allocator.allocate_dma_mem(SIZE_RX_BUFFER);
                                match dma_ptr{
                                        Some(_x) => self.rx_buf_addr[queue][i] = dma_ptr.unwrap(),
                                        None => return Err("rx_init Couldn't allocate DMA mem for rx buffer"),
                                } 

                                let header_ptr = self.nic_dma_allocator.allocate_dma_mem(SIZE_RX_HEADER);
                                let header;
                                match header_ptr{
                                        Some(_x) => header = header_ptr.unwrap(),
                                        None => return Err("rx_init Couldn't allocate DMA mem for rx header"),
                                } 

                                let buf_addr = try!(translate_v2p(self.rx_buf_addr[queue][i]));
                                let header_addr = try!(translate_v2p(header));
                                let mut var : AdvancedReceiveDescriptorR = Default::default(); 
                                var.set_header_buffer_address(0);
                                var.set_packet_buffer_address(buf_addr as u64);
                                                
                                self.rx_descs[queue].push(var);
                        }

                        let slc = self.rx_descs[queue].as_slice(); 
                        let slc_ptr = slc.as_ptr();
                        let v_addr = slc_ptr as usize;
                        debug!("v address of rx_desc: {:x}",v_addr);

                        //let ptr = (translate_v2p(v_addr)).unwrap();
                        let ptr = try!(translate_v2p(v_addr));
                        
                        debug!("p address of rx_desc: {:x}",ptr);
                        let ptr1 = (ptr & 0xFFFF_FFFF) as u32;
                        let ptr2 = (ptr>>32) as u32;

                        self.write_command(REG_RDBAL + (0x40*queue) as u32, ptr1);//lowers bits of 64 bit descriptor base address, 16 byte aligned
                        self.write_command(REG_RDBAH + (0x40*queue) as u32, ptr2);//upper 32 bits
                        
                        self.write_command(REG_RDLEN + (0x40*queue) as u32, (NUM_RX_DESC as u32)* 16);//number of bytes allocated for descriptors, 128 byte aligned
                        
                        self.write_command(REG_RDH + (0x40*queue) as u32, 0);//head pointer for reeive descriptor buffer, points to 16B
                        self.write_command(REG_RDT + (0x40*queue) as u32, 0);//Tail pointer for receive descriptor buffer, point to 16B
                        self.rx_cur[queue] = 0;

                        //set the size of the packet buffers and the descriptor format used
                        let mut val = self.read_command(REG_SRRCTL + (0x40*queue) as u32);
                        val.set_bits(0..4,BSIZEPACKET_8K);
                        val.set_bits(25..27,DESCTYPE_ADV_1BUFFER);
                        self.write_command(REG_SRRCTL + (0x40*queue) as u32, val);

                        //enable the rx queue
                        let mut val = self.read_command(REG_RXDCTL + (0x40*queue) as u32);
                        val.set_bit(25,RX_Q_ENABLE);
                        self.write_command(REG_RXDCTL + (0x40*queue) as u32, val);
                        //make sure queue is enabled
                        while self.read_command(REG_RXDCTL + (0x40*queue) as u32).get_bit(25) != RX_Q_ENABLE {}
                        
                        self.write_command(REG_RDT + (0x40*queue) as u32, NUM_RX_DESC as u32 -1);//Tail pointer for receive descriptor buffer, point to 16B
                
                }
                
               
                self.write_command(REG_FCTRL,0x0000_0702);
                self.write_command(REG_RXCTRL, self.read_command(REG_RXCTRL)|1);
                 Ok(())
        }

        pub fn set_filters(&mut self) {
                let val = self.read_command(REG_ETQF);
                self.write_command(REG_ETQF, val | 0x800 | 0x1000_0000);

                let val = self.read_command(REG_ETQF + 4);
                self.write_command(REG_ETQF + 4, val | 0x800 | 0x1000_0000);

                self.write_command(REG_ETQS, 0x8000_0000);
                self.write_command(REG_ETQS + 4, 0x8001_0000);
        }
        
        pub fn set_rss(&mut self) {
                let mut val = self.read_command(REG_MRQC);
                val.set_bits(0..3,RSS_ONLY);
                val.set_bits(16..31, RSS_UDPIPV4);
                self.write_command(REG_MRQC, val);
        }

        /// Initialize transmit descriptors 
        pub fn tx_init(&mut self) -> Result<(), &'static str>  {
                debug!("HLREG0: {:#X}", self.read_command(REG_HLREG0));

                let val = self.read_command(REG_DMATXCTL);
                self.write_command(REG_DMATXCTL, val & 0xFFFF_FFFE);// set TE (b1) to 0 

                //let val = self.read_command(REG_RTTDCS);
                //self.write_command(REG_RTTDCS,val | 1<<6 ); // set b6 to 1


                const NO_BYTES: usize = 16*(NUM_TX_DESC) + 128;
                
                let dma_ptr = self.nic_dma_allocator.allocate_dma_mem(NO_BYTES);
                let ptr;
                match dma_ptr{
                        Some(_x) => ptr = dma_ptr.unwrap(),
                        None => return Err("ixgbe:tx_init Couldn't allocate DMA mem for tx descriptor"),
                } 

                // make sure memory is 128 byte aligned
                let ptr1 = ptr + (128-(ptr%128));
                debug!("tx pointers: {:#X}, {:#X}",ptr, ptr1);

                let raw_ptr = ptr1 as *mut e1000_tx_desc;
                unsafe{ self.tx_descs = Vec::from_raw_parts(raw_ptr, 0, NUM_TX_DESC);}
                //unsafe{debug!("Address of Rx desc: {:?}, value: {:?}",ptr, *pr1);}
               
                for _i in 0..NUM_TX_DESC
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
                
                self.write_command(REG_TDBAH, ptr2 );
                self.write_command(REG_TDBAL, ptr1);                
                
                //now setup total length of descriptors
                self.write_command(REG_TDLEN, (NUM_TX_DESC as u32) * 16);                
                
                //setup numbers
                self.write_command( REG_TDH, 0);
                self. write_command(REG_TDT,0);
                self.tx_cur = 0;

                let val = self.read_command(REG_DMATXCTL);
                self.write_command(REG_DMATXCTL, val | 1);// set TE (b1) to 1 

                //let val = self.read_command(REG_RTTDCS);
                //self.write_command(REG_RTTDCS,val & 0xFFFF_FFBF); // set b6 to 0
                
                let val = self.read_command(REG_TXDCTL);
                self.write_command(REG_TXDCTL, val | 1<<25); //set b25 to 1

                while self.read_command(REG_TXDCTL)& 1<<25 != 1<<25{} 

                Ok(())
                 
        }  

        /// Send a packet, called by a function higher in the network stack
        /// p_data is address of tranmit buffer, must be pointing to contiguous memory
        pub fn send_packet(&mut self, p_data: usize, p_len: u16) -> Result<(), &'static str> {
                
                //debug!("Value of tx descriptor address_translated: {:x}",ptr);
                /* let t_ptr = translate_v2p(p_data);
                let ptr;
                match t_ptr{
                        Some(_x) => ptr = t_ptr.unwrap(),
                        None => return Err("e1000:send_packet Couldn't translate address for tx buffer"),
                } */ 
                //let ptr = (translate_v2p(p_data)).unwrap();

                let ptr = try!(translate_v2p(p_data));
                
                //debug!("Value of tx descriptor address_translated: {:x}",ptr);
                self.tx_descs[self.tx_cur as usize].addr = ptr as u64;
                self.tx_descs[self.tx_cur as usize].length = p_len;
                self.tx_descs[self.tx_cur as usize].cmd = (CMD_EOP | CMD_IFCS | CMD_RPS | CMD_RS ) as u8; //(1<<0)|(1<<1)|(1<<3)
                self.tx_descs[self.tx_cur as usize].status = 0;

                let old_cur: u8 = self.tx_cur as u8;
                self.tx_cur = (self.tx_cur + 1) % (NUM_TX_DESC as u16);
                debug!("THD {}",self.read_command(REG_TDH));
                debug!("TDT!{}",self.read_command(REG_TDT));
                self. write_command(REG_TDT, self.tx_cur as u32);   
                debug!("THD {}",self.read_command(REG_TDH));
                debug!("TDT!{}",self.read_command(REG_TDT));
                debug!("post-write, tx_descs[{}] = {:?}", old_cur, self.tx_descs[old_cur as usize]);
                debug!("Value of tx descriptor address: {:x}",self.tx_descs[old_cur as usize].addr);
                debug!("Waiting for packet to send!");
                

                while (self.tx_descs[old_cur as usize].status & 0xF) == 0 {
                        //debug!("THD {}",self.read_command(REG_TXDESCHEAD));
                        //debug!("status register: {}",self.tx_descs[old_cur as usize].status);
                }  //bit 0 should be set when done
                debug!("Packet is sent!");  
                Ok(())
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

        pub fn handle_receive_mq(&mut self, queue: usize, mut wb: AdvancedReceiveDescriptorWB) -> Result<(), &'static str> {
                //print status of all packets until EoP
                let mut status = wb.get_ext_status();
                while(status & 0x1) != 0{
                        debug!("rx desc status {:#X}",wb.get_ext_status());

                        let buf_addr = try!(translate_v2p(self.rx_buf_addr[queue][self.rx_cur[queue] as usize]));
                        self.rx_descs[queue][self.rx_cur[queue] as usize].set_packet_buffer_address(buf_addr as u64);
                        self.rx_descs[queue][self.rx_cur[queue] as usize].set_header_buffer_address(0);
                        let old_cur = self.rx_cur[queue] as u32;
                        self.rx_cur[queue] = (self.rx_cur[queue] + 1) % NUM_RX_DESC as u16;
                        self.write_command(REG_RDT + (0x40*queue) as u32, old_cur);
                        wb = AdvancedReceiveDescriptorWB:: from(self.rx_descs[queue][self.rx_cur[queue] as usize]);

                        if (status & 0x2) == 0x2 {
                                break;
                        }

                        status = wb.get_ext_status();
                }

                Ok(())
        }

        fn enable_interrupts(&self) {
                //set IVAR reg for eaach queue used
                self.write_command(REG_IVAR, 0x81); // for rxq 0
                debug!("IVAR: {:#X}", self.read_command(REG_IVAR));
                
                //enable clear on read of EICR
                self.write_command(REG_GPIE, (self.read_command(REG_GPIE) & 0xFFFFFFDF) | 0x40); //bit 5
                debug!("GPIE: {:#X}", self.read_command(REG_GPIE));

                //clears eicr by writing 1 to clear old interrupt causes
                self.read_command(REG_EICR);
                debug!("EICR: {:#X}", self.read_command(REG_EICR));

                //set eims to enable required interrupt
                self.write_command(REG_EIMS, 0xFFFF);
                debug!("EIMS: {:#X}", self.read_command(REG_EIMS));
        }


        fn handle_interrupt(&self) {
                debug!("In handle_interrupt");

                let status = self.read_command(REG_EICR); //reads status and clears interrupt
                if (status & 0x01 ) == 0x01 { //Rx0
                        debug!("Interrupt:packet received");

                }
                else{
                        debug!("Unhandled interrupt!");
                }
                
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
                        rx_descs: [Vec::with_capacity(NUM_RX_DESC)],// Vec::with_capacity(NUM_RX_DESC), Vec::with_capacity(NUM_RX_DESC), Vec::with_capacity(NUM_RX_DESC), Vec::with_capacity(NUM_RX_DESC), Vec::with_capacity(NUM_RX_DESC), Vec::with_capacity(NUM_RX_DESC), Vec::with_capacity(NUM_RX_DESC), Vec::with_capacity(NUM_RX_DESC), Vec::with_capacity(NUM_RX_DESC), Vec::with_capacity(NUM_RX_DESC), Vec::with_capacity(NUM_RX_DESC), Vec::with_capacity(NUM_RX_DESC), Vec::with_capacity(NUM_RX_DESC), Vec::with_capacity(NUM_RX_DESC), Vec::with_capacity(NUM_RX_DESC)],
                        tx_descs: Vec::with_capacity(NUM_TX_DESC),
                        rx_cur: [0; NO_RX_QUEUES],
                        tx_cur: 0,
                        rx_buf_addr: [[0;NUM_RX_DESC]],// [0;NUM_RX_DESC], [0;NUM_RX_DESC], [0;NUM_RX_DESC], [0;NUM_RX_DESC], [0;NUM_RX_DESC], [0;NUM_RX_DESC], [0;NUM_RX_DESC], [0;NUM_RX_DESC], [0;NUM_RX_DESC], [0;NUM_RX_DESC], [0;NUM_RX_DESC], [0;NUM_RX_DESC], [0;NUM_RX_DESC], [0;NUM_RX_DESC], [0;NUM_RX_DESC]],
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
        
        INTERRUPT_NO.call_once(|| pci_read_8(dev_pci.bus, dev_pci.slot, dev_pci.func, PCI_INTERRUPT_LINE) );
        debug!("Int line: {}  Int pin: {}" , *INTERRUPT_NO.try().unwrap_or(&0), pci_read_8(dev_pci.bus, dev_pci.slot, dev_pci.func, PCI_INTERRUPT_PIN) );

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
        debug!("AUTOC: {:#X}", nic.read_command(REG_AUTOC)); 

        let mut val = nic.read_command(REG_AUTOC);
        val = val & !(0x0000_E000) & !(0x0000_0200);
        nic.write_command(REG_AUTOC, val|AUTOC_10G_PMA_PMD_PAR|AUTOC_FLU);

        let mut val = nic.read_command(REG_AUTOC2);
        val = val & !(0x0003_0000);
        nic.write_command(REG_AUTOC2, val|1<<17);

        let val = nic.read_command(REG_AUTOC);
        nic.write_command(REG_AUTOC, val|AUTOC_RESTART_AN); 


        debug!("STATUS: {:#X}", nic.read_command(REG_STATUS)); 
        debug!("CTRL: {:#X}", nic.read_command(REG_CTRL));
        debug!("LINKS: {:#X}", nic.read_command(REG_LINKS)); //b7 and b30 should be 1 for link up 
        debug!("AUTOC: {:#X}", nic.read_command(REG_AUTOC)); 

        //Initialize statistics

        //Enable Interrupts
        nic.enable_interrupts();
        //register_interrupt(*INTERRUPT_NO.try().unwrap() + 32, ixgbe_handler);
        
        //Initialize transmit .. legacy descriptors

        try!(nic.tx_init());
        debug!("TXDCTL: {:#X}", nic.read_command(REG_TXDCTL)); //b25 should be 1 for tx queue enable

        //initalize receive... legacy descriptors
        try!(nic.rx_init_mq());

        //nic.set_filters();
        // nic.set_rss();

       Ok(())
}

/* pub fn rx_poll(_: Option<u64>){
        //debug!("e1000e poll function");
        loop {
                let mut nic = NIC_82599.lock();

        //detect if a packet has beem received
        //debug!("E1000E_RX POLL");
        /* for i in 0..NUM_RX_DESC {
                debug!("rx desc status {}",self.rx_descs[i].status);
        } */
        //debug!("E1000E RCTL{:#X}",self.read_command(REG_RCTRL));
        
                if (nic.rx_descs[nic.rx_cur as usize].status&0xF) != 0 {    
                        debug!("Packet received!");                    
                        nic.handle_receive();
                }
        }
        
} */

pub fn rx_poll_mq(_: Option<u64>){
        //debug!("e1000e poll function");
        loop {
                let mut nic = NIC_82599.lock();

       
                for queue in 0..NO_RX_QUEUES{
                        let mut a = AdvancedReceiveDescriptorWB:: from(nic.rx_descs[queue][nic.rx_cur[queue] as usize]);
                        if (a.get_ext_status()&0x1) != 0 {    
                                debug!("Packet received in QUEUE{}!", queue);
                                let _ = nic.handle_receive_mq(queue, a);                    
                        }       
                }        
                   
        }
        
}

// extern "x86-interrupt" fn ixgbe_handler(_stack_frame: &mut ExceptionStackFrame) {
//     let nic = NIC_82599.lock();
//     debug!("nic handler called");
//     nic.handle_interrupt();
//     eoi(Some(*(INTERRUPT_NO.try().unwrap())));
// }

pub fn check_eicr(_ : Option<u64>){
        loop {
                let nic = NIC_82599.lock();
                debug!("EICR: {:#X}", nic.read_command(REG_EICR));
        }
}

pub fn cause_interrupt(_ : Option<u64>) {
        let nic = NIC_82599.lock();
        nic.write_command(REG_EICS,0xFFFF);
}

pub fn ixgbe_handler() {
        let nic = NIC_82599.lock();
        nic.handle_interrupt();
}

pub fn pci_config_space(dev_pci: &PciDevice) {

        debug!("NIC PCI CONFIG SPACE");

        let status = pci_read_16(dev_pci.bus, dev_pci.slot, dev_pci.func, PCI_STATUS);

        debug!("status: {:#X}", status);

        if (status >> 4 & 1) == 1 {
                let capabilities = pci_read_8(dev_pci.bus, dev_pci.slot, dev_pci.func, PCI_CAPABILITIES);
                debug!("capabilities pointer: {:#X}", capabilities);

                let mut node= pci_read_16(dev_pci.bus, dev_pci.slot, dev_pci.func, capabilities as u16 & 0xFFFC);
                let mut node_next= 1;
                let mut node_id;

                while node_next != 0 {

                        node_id = node & 0xFF;
                        

                        if node_id == MSI_CAPABILITY {
                                let msi_control = pci_read_32(dev_pci.bus, dev_pci.slot, dev_pci.func, node_next);

                                debug!("msi control: {:#X}", msi_control);// (msi_control>>16) & 0xFFFF);
                        }

                        node_next = (node >> 8) & 0xFF;

                        debug!("node_id: {:#X}, node_next: {:#X}", node_id, node_next);
                        
                        node= pci_read_16(dev_pci.bus, dev_pci.slot, dev_pci.func, node_next as u16);

                }
        }

        

}
