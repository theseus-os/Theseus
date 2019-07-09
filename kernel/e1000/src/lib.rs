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
extern crate apic;
extern crate intel_ethernet;
extern crate nic_buffers;
extern crate nic_queues;
extern crate nic_initialization;

pub mod test_e1000_driver;
mod regs;


use spin::Once; 
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use irq_safety::{RwLockIrqSafe, MutexIrqSafe};
use volatile::{Volatile, ReadOnly};
use alloc::boxed::Box;
use memory::{PhysicalAddress, VirtualAddress, MappedPages};
use pci::{PciDevice, PCI_BAR0, PCI_INTERRUPT_LINE, PciConfigSpaceAccessMechanism};
use kernel_config::memory::PAGE_SIZE;
use owning_ref::BoxRefMut;
use interrupts::{eoi,register_interrupt};
use x86_64::structures::idt::{ExceptionStackFrame};
use network_interface_card:: NetworkInterfaceCard;
use nic_initialization::{allocate_device_register_memory, init_rx_buf_pool, init_rx_queue, init_tx_queue};
use intel_ethernet::{
    descriptors::{LegacyRxDescriptor, LegacyTxDescriptor},
    types::*
};
use nic_buffers::{TransmitBuffer, ReceiveBuffer, ReceivedFrame};
use nic_queues::{RxQueue, TxQueue};
use apic::get_my_apic_id;
use regs:: {REG_RXDESCTAIL, REG_TXDESCTAIL};

pub const INTEL_VEND:           u16 = 0x8086;  // Vendor ID for Intel 
pub const E1000_DEV:            u16 = 0x100E;  // Device ID for the e1000 Qemu, Bochs, and VirtualBox emmulated NICs

const E1000_NUM_RX_DESC:        usize = 8;
const E1000_NUM_TX_DESC:        usize = 8;

/// Currently, each receive buffer is a single page.
const E1000_RX_BUFFER_SIZE_IN_BYTES:     u16 = PAGE_SIZE as u16;

/// Interrupt type: Link Status Change
const INT_LSC:              u32 = 0x04;
/// Interrupt type: Receive Timer Interrupt
const INT_RX:               u32 = 0x80;


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
pub struct E1000Registers {
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

    pub rx_regs:                    RegistersRx,            // 0x2800    
    _padding6:                      [u8; 4068],             // 0x281C - 0x37FF

    pub tx_regs:                    RegistersTx,            // 0x3800
    _padding7:                      [u8; 7140],             // 0x381C - 0x53FF
    
    /// The lower (least significant) 32 bits of the NIC's MAC hardware address.
    pub ral:                        Volatile<u32>,          // 0x5400
    /// The higher (most significant) 32 bits of the NIC's MAC hardware address.
    pub rah:                        Volatile<u32>,          // 0x5404
    _padding8:                      [u8; 109560],           // 0x5408 - 0x1FFFF END: 0x20000 (128 KB) ..116708
}

///struct to hold registers related to one receive queue
#[repr(C)]
pub struct RegistersRx {
    /// The lower (least significant) 32 bits of the physical address of the array of receive descriptors.
    pub rdbal:                      Volatile<Rdbal>,        // 0x2800
    /// The higher (most significant) 32 bits of the physical address of the array of receive descriptors.
    pub rdbah:                      Volatile<Rdbah>,        // 0x2804
    /// The length in bytes of the array of receive descriptors.
    pub rdlen:                      Volatile<Rdlen>,        // 0x2808
    _padding0:                      [u8; 4],                // 0x280C - 0x280F
    /// The receive descriptor head index, which points to the next available receive descriptor.
    pub rdh:                        Volatile<Rdh>,          // 0x2810
    _padding1:                      [u8; 4],                // 0x2814 - 0x2817
    /// The receive descriptor tail index, which points to the last available receive descriptor.
    pub rdt:                        Volatile<Rdt>,          // 0x2818
}


///struct to hold registers related to one transmit queue
#[repr(C)]
pub struct RegistersTx {
    /// The lower (least significant) 32 bits of the physical address of the array of transmit descriptors.
    pub tdbal:                      Volatile<Tdbal>,        // 0x3800
    /// The higher (most significant) 32 bits of the physical address of the array of transmit descriptors.
    pub tdbah:                      Volatile<Tdbah>,        // 0x3804
    /// The length in bytes of the array of transmit descriptors.
    pub tdlen:                      Volatile<Tdlen>,        // 0x3808
    _padding0:                      [u8; 4],                // 0x380C - 0x380F
    /// The transmit descriptor head index, which points to the next available transmit descriptor.
    pub tdh:                        Volatile<Tdh>,          // 0x3810
    _padding1:                      [u8; 4],                // 0x3814 - 0x3817
    /// The transmit descriptor tail index, which points to the last available transmit descriptor.
    pub tdt:                        Volatile<Tdt>,          // 0x3818
}


/// A struct representing an e1000 network interface card.
pub struct E1000Nic {
    /// Type of BAR0
    bar_type: u8,
    /// MMIO Base Address
    mem_base: PhysicalAddress,
    ///interrupt number
    interrupt_num: u8,
    /// The actual MAC address burnt into the hardware of this E1000 NIC.
    mac_hardware: [u8; 6],
    /// The optional spoofed MAC address to use in place of `mac_hardware` when transmitting.  
    mac_spoofed: Option<[u8; 6]>,
    /// Receive queue with descriptors
    rx_queue: RxQueue<LegacyRxDescriptor>,
    /// Transmit queue with descriptors
    tx_queue: TxQueue<LegacyTxDescriptor>,     
    /// memory-mapped control registers
    regs: BoxRefMut<MappedPages, E1000Registers>,
}


impl NetworkInterfaceCard for E1000Nic {

    fn send_packet(&mut self, transmit_buffer: TransmitBuffer) -> Result<(), &'static str> {
        Self::send_on_queue(&mut self.tx_queue, E1000_NUM_TX_DESC as u16, transmit_buffer);
        Ok(())
    }

    fn get_received_frame(&mut self) -> Option<ReceivedFrame> {
        self.rx_queue.received_frames.pop_front()
    }

    fn poll_receive(&mut self) -> Result<(), &'static str> {
        Self::remove_frames_from_queue(&mut self.rx_queue, E1000_NUM_RX_DESC as u16, &RX_BUFFER_POOL, E1000_RX_BUFFER_SIZE_IN_BYTES)        
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
        let interrupt_num = e1000_pci_dev.pci_read_8(PCI_INTERRUPT_LINE) + PIC_MASTER_OFFSET;
        // debug!("e1000 IRQ number: {}", interrupt_num);

        let bar0 = e1000_pci_dev.bars[0];
        // Determine the access mechanism from the base address register's bit 0
        let bar_type = (bar0 as u8) & 0x1;    

        // If the base address is not memory mapped then exit
        if bar_type == PciConfigSpaceAccessMechanism::IoPort as u8 {
            error!("e1000::init(): BAR0 is of I/O type");
            return Err("e1000::init(): BAR0 is of I/O type")
        }
  
        // memory mapped base address
        let mem_base = e1000_pci_dev.determine_mem_base()?;

        // set the bus mastering bit for this PciDevice, which allows it to use DMA
        e1000_pci_dev.pci_set_command_bus_master_bit();

        let (mut mapped_registers, mem_base_v) = Self::map_e1000_regs(e1000_pci_dev, mem_base)?;
        
        Self::start_link(&mut mapped_registers);
        
        let mac_addr_hardware = Self::read_mac_address_from_nic(&mut mapped_registers);
        //e1000_nc.clear_multicast();
        //e1000_nc.clear_statistics();
        
        Self::enable_interrupts(&mut mapped_registers);
        register_interrupt(interrupt_num, e1000_handler)?;

        // initialize the buffer pool
        init_rx_buf_pool(RX_BUFFER_POOL_SIZE, E1000_RX_BUFFER_SIZE_IN_BYTES, &RX_BUFFER_POOL)?;

        let (rx_descs, rx_buffers) = Self::rx_init(&mut mapped_registers)?;
        let rxq = RxQueue {
            id: 0,
            rx_descs: rx_descs,
            rx_cur: 0,
            rx_bufs_in_use: rx_buffers,
            received_frames: VecDeque::new(),
            // here the cpu id is irrelevant because there's no DCA or MSI 
            cpu_id: get_my_apic_id().ok_or("E1000::init(): couldn't get my apic id")?,
            rdt_addr: VirtualAddress::new(mem_base_v.value() + REG_RXDESCTAIL as usize)?,
        };

        let tx_descs = Self::tx_init(&mut mapped_registers)?;
        let txq = TxQueue {
            id: 0,
            tx_descs: tx_descs,
            tx_cur: 0,
            cpu_id: get_my_apic_id().ok_or("E1000::init(): couldn't get my apic id")?,
            tdt_addr: VirtualAddress::new(mem_base_v.value() + REG_TXDESCTAIL as usize)?,
        };

        let e1000_nic = E1000Nic {
            bar_type: bar_type,
            mem_base: mem_base,
            interrupt_num: interrupt_num,
            mac_hardware: mac_addr_hardware,
            mac_spoofed: None,
            rx_queue: rxq,
            tx_queue: txq,
            regs: mapped_registers,
        };
        
        let nic_ref = E1000_NIC.call_once(|| MutexIrqSafe::new(e1000_nic));
        Ok(nic_ref)
    }
    
    /// Allocates memory for the NIC and maps the E1000 Register struct to that memory area.
    /// Returns a reference to the mapped E1000 Registers and the virtual address where the registers are mapped.
    /// 
    /// # Arguments
    /// * `device`: reference to the nic device
    /// * `mem_base`: the physical address where the NIC's memory starts.
    fn map_e1000_regs(device: &PciDevice, mem_base: PhysicalAddress) -> Result<(BoxRefMut<MappedPages, E1000Registers>, VirtualAddress), &'static str> {
        let nic_mapped_page = allocate_device_register_memory(device, mem_base)?;
        let mem_base_v = nic_mapped_page.start_address();
        let regs = BoxRefMut::new(Box::new(nic_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<E1000Registers>(0))?;
            
        Ok((regs, mem_base_v))
    }

    pub fn spoof_mac(&mut self, spoofed_mac_addr: [u8; 6]) {
        self.mac_spoofed = Some(spoofed_mac_addr);
    }

    /// Reads the actual MAC address burned into the NIC hardware.
    fn read_mac_address_from_nic(regs: &mut E1000Registers) -> [u8; 6] {
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
    fn start_link(regs: &mut E1000Registers) {
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
    fn rx_init(regs: &mut E1000Registers) -> Result<(BoxRefMut<MappedPages, [LegacyRxDescriptor]>, Vec<ReceiveBuffer>), &'static str> {


        // get the queue of rx descriptors and its corresponding rx buffers
        // let (rx_descs, rx_bufs_in_use) = Self::init_rx_queue(E1000_NUM_RX_DESC, &RX_BUFFER_POOL, E1000_RX_BUFFER_SIZE_IN_BYTES as usize, &mut regs.rdbal, 
        //                                 &mut regs.rdbah, &mut regs.rdlen, &mut regs.rdh, &mut regs.rdt)?;          
        
        let (rx_descs, rx_bufs_in_use) = init_rx_queue(E1000_NUM_RX_DESC, &RX_BUFFER_POOL, E1000_RX_BUFFER_SIZE_IN_BYTES as usize, &mut regs.rx_regs.rdbal,
                                            &mut regs.rx_regs.rdbah, &mut regs.rx_regs.rdlen, &mut regs.rx_regs.rdt, &mut regs.rx_regs.rdh)?;          
            
        // Write the tail index.
        // Note that the e1000 SDM states that we should set the RDT (tail index) to the index *beyond* the last receive descriptor, 
        // so if you have 8 rx descs, you will set it to 8. 
        // However, this causes problems during the first burst of ethernet packets when you first enable interrupts, 
        // because the `rx_cur` counter won't be able to catch up with the head index properly. 
        // Thus, we set it to one less than that in order to prevent such bugs. 
        // This doesn't prevent all of the rx buffers from being used, they will still all be used fully.
        regs.rx_regs.rdt.write((E1000_NUM_RX_DESC - 1) as u32); 
        // TODO: document these various e1000 flags and why we're setting them
        regs.rctl.write(regs::RCTL_EN| regs::RCTL_SBP | regs::RCTL_LBM_NONE | regs::RTCL_RDMTS_HALF | regs::RCTL_BAM | regs::RCTL_SECRC  | regs::RCTL_BSIZE_2048);

        Ok((rx_descs, rx_bufs_in_use))
    }           
    
    /// Initialize the array of tramsmit descriptors and return them.
    fn tx_init(regs: &mut E1000Registers) -> Result<BoxRefMut<MappedPages, [LegacyTxDescriptor]>, &'static str> {

        let tx_descs = init_tx_queue(E1000_NUM_TX_DESC, &mut regs.tx_regs.tdbal, &mut regs.tx_regs.tdbah, &mut regs.tx_regs.tdlen, &mut regs.tx_regs.tdt, &mut regs.tx_regs.tdh)?;
        
        regs.tctl.write(regs::TCTL_EN | regs::TCTL_PSP);

        Ok(tx_descs)
    }       
    
    /// Enable Interrupts 
    fn enable_interrupts(regs: &mut E1000Registers) {
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
        Self::remove_frames_from_queue(&mut self.rx_queue, E1000_NUM_RX_DESC as u16, &RX_BUFFER_POOL, E1000_RX_BUFFER_SIZE_IN_BYTES)        
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
