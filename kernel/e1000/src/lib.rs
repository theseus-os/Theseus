#![no_std]

#![allow(dead_code)] //  to suppress warnings for unused functions/methods
#![feature(rustc_private)]
#![feature(abi_x86_interrupt)]

#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate static_assertions;
extern crate volatile;
extern crate zerocopy;
extern crate alloc;
extern crate spin;
extern crate irq_safety;
extern crate kernel_config;
extern crate memory;
extern crate pci; 
extern crate owning_ref;
extern crate interrupts;
extern crate x86_64;
extern crate mpmc;
extern crate network_interface_card;
extern crate intel_ethernet;
extern crate nic_buffers;
extern crate nic_queues;
extern crate nic_initialization;

pub mod test_e1000_driver;
mod regs;
use regs::*;

use spin::Once; 
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use irq_safety::MutexIrqSafe;
use alloc::boxed::Box;
use memory::{PhysicalAddress, MappedPages};
use pci::{PciDevice, PCI_INTERRUPT_LINE, PciConfigSpaceAccessMechanism};
use kernel_config::memory::PAGE_SIZE;
use owning_ref::BoxRefMut;
use interrupts::{eoi, register_interrupt};
use x86_64::structures::idt::InterruptStackFrame;
use network_interface_card:: NetworkInterfaceCard;
use nic_initialization::{allocate_memory, init_rx_buf_pool, init_rx_queue, init_tx_queue};
use intel_ethernet::descriptors::{LegacyRxDescriptor, LegacyTxDescriptor};
use nic_buffers::{TransmitBuffer, ReceiveBuffer, ReceivedFrame};
use nic_queues::{RxQueue, TxQueue, RxQueueRegisters, TxQueueRegisters};

pub const INTEL_VEND:           u16 = 0x8086;  // Vendor ID for Intel 
pub const E1000_DEV:            u16 = 0x100E;  // Device ID for the e1000 Qemu, Bochs, and VirtualBox emmulated NICs

const E1000_NUM_RX_DESC:        u16 = 8;
const E1000_NUM_TX_DESC:        u16 = 8;

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
    E1000_NIC.get()
}

/// How many ReceiveBuffers are preallocated for this driver to use. 
const RX_BUFFER_POOL_SIZE: usize = 256; 
lazy_static! {
    /// The pool of pre-allocated receive buffers that are used by the E1000 NIC
    /// and temporarily given to higher layers in the networking stack.
    static ref RX_BUFFER_POOL: mpmc::Queue<ReceiveBuffer> = mpmc::Queue::with_capacity(RX_BUFFER_POOL_SIZE);
}


/// A struct which contains the receive queue registers and implements the `RxQueueRegisters` trait,
/// which is required to store the registers in an `RxQueue` object.
struct E1000RxQueueRegisters(BoxRefMut<MappedPages, E1000RxRegisters>);

impl RxQueueRegisters for E1000RxQueueRegisters {
    fn set_rdbal(&mut self, value: u32) {
        self.0.rx_regs.rdbal.write(value); 
    }    
    fn set_rdbah(&mut self, value: u32) {
        self.0.rx_regs.rdbah.write(value); 
    }
    fn set_rdlen(&mut self, value: u32) {
        self.0.rx_regs.rdlen.write(value); 
    }
    fn set_rdh(&mut self, value: u32) {
        self.0.rx_regs.rdh.write(value); 
    }
    fn set_rdt(&mut self, value: u32) {
        self.0.rx_regs.rdt.write(value); 
    }
} 

/// A struct which contains the transmit queue registers and implements the `TxQueueRegisters` trait,
/// which is required to store the registers in a `TxQueue` object.
struct E1000TxQueueRegisters(BoxRefMut<MappedPages, E1000TxRegisters>);

impl TxQueueRegisters for E1000TxQueueRegisters {
    fn set_tdbal(&mut self, value: u32) {
        self.0.tx_regs.tdbal.write(value); 
    }    
    fn set_tdbah(&mut self, value: u32) {
        self.0.tx_regs.tdbah.write(value); 
    }
    fn set_tdlen(&mut self, value: u32) {
        self.0.tx_regs.tdlen.write(value); 
    }
    fn set_tdh(&mut self, value: u32) {
        self.0.tx_regs.tdh.write(value); 
    }
    fn set_tdt(&mut self, value: u32) {
        self.0.tx_regs.tdt.write(value); 
    }
}

/// Struct representing an e1000 network interface card.
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
    rx_queue: RxQueue<E1000RxQueueRegisters,LegacyRxDescriptor>,
    /// Transmit queue with descriptors
    tx_queue: TxQueue<E1000TxQueueRegisters,LegacyTxDescriptor>,     
    /// memory-mapped control registers
    regs: BoxRefMut<MappedPages, E1000Registers>,
    /// memory-mapped registers holding the MAC address
    mac_regs: BoxRefMut<MappedPages, E1000MacRegisters>
}


impl NetworkInterfaceCard for E1000Nic {

    fn send_packet(&mut self, transmit_buffer: TransmitBuffer) -> Result<(), &'static str> {
        self.tx_queue.send_on_queue(transmit_buffer);
        Ok(())
    }

    fn get_received_frame(&mut self) -> Option<ReceivedFrame> {
        self.rx_queue.received_frames.pop_front()
    }

    fn poll_receive(&mut self) -> Result<(), &'static str> {
        self.rx_queue.poll_queue_and_store_received_packets()  
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac_spoofed.unwrap_or(self.mac_hardware)
    }
}



/// Functions that setup the NIC struct and handle the sending and receiving of packets.
impl E1000Nic {
    /// Initializes the new E1000 network interface card that is connected as the given PciDevice.
    pub fn init(e1000_pci_dev: &PciDevice) -> Result<&'static MutexIrqSafe<E1000Nic>, &'static str> {
        use interrupts::IRQ_BASE_OFFSET;

        //debug!("e1000_nc bar_type: {0}, mem_base: {1}, io_base: {2}", e1000_nc.bar_type, e1000_nc.mem_base, e1000_nc.io_base);
        
        // Get interrupt number
        let interrupt_num = e1000_pci_dev.pci_read_8(PCI_INTERRUPT_LINE) + IRQ_BASE_OFFSET;
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
        let mem_base = e1000_pci_dev.determine_mem_base(0)?;

        // set the bus mastering bit for this PciDevice, which allows it to use DMA
        e1000_pci_dev.pci_set_command_bus_master_bit();

        let (mut mapped_registers, rx_registers, tx_registers, mut mac_registers)  = Self::map_e1000_regs(e1000_pci_dev, mem_base)?;
        let mut rx_registers =  E1000RxQueueRegisters(rx_registers);
        let mut tx_registers =  E1000TxQueueRegisters(tx_registers);

        Self::start_link(&mut mapped_registers);
        
        let mac_addr_hardware = Self::read_mac_address_from_nic(&mut mac_registers);
        //e1000_nc.clear_multicast();
        //e1000_nc.clear_statistics();
        
        Self::enable_interrupts(&mut mapped_registers);
        register_interrupt(interrupt_num, e1000_handler).map_err(|_handler_addr| {
            error!("e1000 IRQ {:#X} was already in use by handler {:#X}! Sharing IRQs is currently unsupported.", interrupt_num, _handler_addr);
            "e1000 interrupt number was already in use! Sharing IRQs is currently unsupported."
        })?;

        // initialize the buffer pool
        init_rx_buf_pool(RX_BUFFER_POOL_SIZE, E1000_RX_BUFFER_SIZE_IN_BYTES, &RX_BUFFER_POOL)?;

        let (rx_descs, rx_buffers) = Self::rx_init(&mut mapped_registers, &mut rx_registers)?;
        let rxq = RxQueue {
            id: 0,
            regs: rx_registers,
            rx_descs: rx_descs,
            num_rx_descs: E1000_NUM_RX_DESC,
            rx_cur: 0,
            rx_bufs_in_use: rx_buffers,
            rx_buffer_size_bytes: E1000_RX_BUFFER_SIZE_IN_BYTES,
            received_frames: VecDeque::new(),
            // here the cpu id is irrelevant because there's no DCA or MSI 
            cpu_id: None,
            rx_buffer_pool: &RX_BUFFER_POOL,
            filter_num: None
        };

        let tx_descs = Self::tx_init(&mut mapped_registers, &mut tx_registers)?;
        let txq = TxQueue {
            id: 0,
            regs: tx_registers,
            tx_descs: tx_descs,
            num_tx_descs: E1000_NUM_TX_DESC,
            tx_cur: 0,
            cpu_id: None,
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
            mac_regs: mac_registers
        };
        
        let nic_ref = E1000_NIC.call_once(|| MutexIrqSafe::new(e1000_nic));
        Ok(nic_ref)
    }
    
    /// Allocates memory for the NIC and maps the E1000 Register struct to that memory area.
    /// Returns a reference to the E1000 Registers, tied to their backing `MappedPages`.
    /// 
    /// # Arguments
    /// * `device`: reference to the nic device
    /// * `mem_base`: the physical address where the NIC's memory starts.
    fn map_e1000_regs(
        _device: &PciDevice, 
        mem_base: PhysicalAddress
    ) -> Result<(
        BoxRefMut<MappedPages, E1000Registers>, 
        BoxRefMut<MappedPages, E1000RxRegisters>, 
        BoxRefMut<MappedPages, E1000TxRegisters>, 
        BoxRefMut<MappedPages, E1000MacRegisters>
    ), &'static str> {

        const GENERAL_REGISTERS_SIZE_BYTES: usize = 8192;
        const RX_REGISTERS_SIZE_BYTES: usize = 4096;
        const TX_REGISTERS_SIZE_BYTES: usize = 4096;
        const MAC_REGISTERS_SIZE_BYTES: usize = 114_688;

        let nic_regs_mapped_page = allocate_memory(mem_base, GENERAL_REGISTERS_SIZE_BYTES)?;
        let nic_rx_regs_mapped_page = allocate_memory(mem_base + GENERAL_REGISTERS_SIZE_BYTES, RX_REGISTERS_SIZE_BYTES)?;
        let nic_tx_regs_mapped_page = allocate_memory(mem_base + GENERAL_REGISTERS_SIZE_BYTES + RX_REGISTERS_SIZE_BYTES, TX_REGISTERS_SIZE_BYTES)?;
        let nic_mac_regs_mapped_page = allocate_memory(mem_base + GENERAL_REGISTERS_SIZE_BYTES + RX_REGISTERS_SIZE_BYTES + TX_REGISTERS_SIZE_BYTES, MAC_REGISTERS_SIZE_BYTES)?;

        let regs = BoxRefMut::new(Box::new(nic_regs_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<E1000Registers>(0))?;
        let rx_regs = BoxRefMut::new(Box::new(nic_rx_regs_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<E1000RxRegisters>(0))?;
        let tx_regs = BoxRefMut::new(Box::new(nic_tx_regs_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<E1000TxRegisters>(0))?;
        let mac_regs = BoxRefMut::new(Box::new(nic_mac_regs_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<E1000MacRegisters>(0))?;

        Ok((regs, rx_regs, tx_regs, mac_regs))
    }

    pub fn spoof_mac(&mut self, spoofed_mac_addr: [u8; 6]) {
        self.mac_spoofed = Some(spoofed_mac_addr);
    }

    /// Reads the actual MAC address burned into the NIC hardware.
    fn read_mac_address_from_nic(regs: &mut E1000MacRegisters) -> [u8; 6] {
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
    fn rx_init(
        regs: &mut E1000Registers, 
        rx_regs: &mut E1000RxQueueRegisters
    ) -> Result<(
        BoxRefMut<MappedPages, [LegacyRxDescriptor]>, 
        Vec<ReceiveBuffer>
    ), &'static str> {
        // get the queue of rx descriptors and its corresponding rx buffers     
        let (rx_descs, rx_bufs_in_use) = init_rx_queue(E1000_NUM_RX_DESC as usize, &RX_BUFFER_POOL, E1000_RX_BUFFER_SIZE_IN_BYTES as usize, rx_regs)?;          
            
        // Write the tail index.
        // Note that the e1000 SDM states that we should set the RDT (tail index) to the index *beyond* the last receive descriptor, 
        // so if you have 8 rx descs, you will set it to 8. 
        // However, this causes problems during the first burst of ethernet packets when you first enable interrupts, 
        // because the `rx_cur` counter won't be able to catch up with the head index properly. 
        // Thus, we set it to one less than that in order to prevent such bugs. 
        // This doesn't prevent all of the rx buffers from being used, they will still all be used fully.
        rx_regs.set_rdt((E1000_NUM_RX_DESC - 1) as u32); 
        // TODO: document these various e1000 flags and why we're setting them
        regs.rctl.write(regs::RCTL_EN| regs::RCTL_SBP | regs::RCTL_LBM_NONE | regs::RTCL_RDMTS_HALF | regs::RCTL_BAM | regs::RCTL_SECRC  | regs::RCTL_BSIZE_2048);

        Ok((rx_descs, rx_bufs_in_use))
    }           
    
    /// Initialize the array of tramsmit descriptors and return them.
    fn tx_init(
        regs: &mut E1000Registers, 
        tx_regs: &mut E1000TxQueueRegisters
    ) -> Result<BoxRefMut<MappedPages, [LegacyTxDescriptor]>, &'static str> {
        // get the queue of tx descriptors     
        let tx_descs = init_tx_queue(E1000_NUM_TX_DESC as usize, tx_regs)?;
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
            self.poll_receive()?;
            handled = true;
        }

        if !handled {
            error!("e1000::handle_interrupt(): unhandled interrupt!  status: {:#X}", status);
        }
        //regs.icr.read(); //clear interrupt
        Ok(())
    }
}

extern "x86-interrupt" fn e1000_handler(_stack_frame: InterruptStackFrame) {
    if let Some(ref e1000_nic_ref) = E1000_NIC.get() {
        let mut e1000_nic = e1000_nic_ref.lock();
        if let Err(e) = e1000_nic.handle_interrupt() {
            error!("e1000_handler(): error handling interrupt: {:?}", e);
        }
        eoi(Some(e1000_nic.interrupt_num));
    } else {
        error!("BUG: e1000_handler(): E1000 NIC hasn't yet been initialized!");
    }

}
