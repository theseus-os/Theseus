#![no_std]

#![allow(clippy::type_complexity)]
#![allow(dead_code)] //  to suppress warnings for unused functions/methods
#![feature(rustc_private)]
#![feature(abi_x86_interrupt)]

#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
extern crate volatile;
extern crate zerocopy;
extern crate alloc;
extern crate spin;
extern crate sync_irq;
extern crate kernel_config;
extern crate memory;
extern crate pci; 
extern crate interrupts;
extern crate x86_64;
extern crate mpmc;
extern crate intel_ethernet;
extern crate nic_buffers;
extern crate nic_queues;
extern crate nic_initialization;
extern crate net;
extern crate deferred_interrupt_tasks;
extern crate task;

pub mod test_e1000_driver;
mod regs;
use regs::*;

use spin::Once; 
use alloc::{collections::VecDeque, format, sync::Arc, vec::Vec};
use sync_irq::IrqSafeMutex;
use memory::{PhysicalAddress, BorrowedMappedPages, BorrowedSliceMappedPages, Mutable, map_frame_range, MMIO_FLAGS};
use pci::{PciDevice, PciConfigSpaceAccessMechanism};
use kernel_config::memory::PAGE_SIZE;
use interrupts::{eoi, InterruptNumber};
use x86_64::structures::idt::InterruptStackFrame;
use nic_initialization::{init_rx_buf_pool, init_rx_queue, init_tx_queue};
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
static E1000_NIC: Once<IrqSafeMutex<E1000Nic>> = Once::new();

/// Returns a reference to the E1000Nic wrapped in a IrqSafeMutex,
/// if it exists and has been initialized.
pub fn get_e1000_nic() -> Option<&'static IrqSafeMutex<E1000Nic>> {
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
struct E1000RxQueueRegisters(BorrowedMappedPages<E1000RxRegisters, Mutable>);

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
struct E1000TxQueueRegisters(BorrowedMappedPages<E1000TxRegisters, Mutable>);

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
    /// The interrupt vector number used by this device to trigger interrupts.
    interrupt_num: InterruptNumber,
    /// The actual MAC address burnt into the hardware of this E1000 NIC.
    mac_hardware: [u8; 6],
    /// The optional spoofed MAC address to use in place of `mac_hardware` when transmitting.  
    mac_spoofed: Option<[u8; 6]>,
    /// MMIO Base Address
    mem_base: PhysicalAddress,
    /// Receive queue with descriptors
    rx_queue: RxQueue<E1000RxQueueRegisters, LegacyRxDescriptor>,
    /// Transmit queue with descriptors
    tx_queue: TxQueue<E1000TxQueueRegisters, LegacyTxDescriptor>,     
    /// memory-mapped control registers
    regs: BorrowedMappedPages<E1000Registers, Mutable>,
    /// memory-mapped registers holding the MAC address
    mac_regs: BorrowedMappedPages<E1000MacRegisters, Mutable>,
    deferred_task: Option<task::JoinableTaskRef>,
}

/// Functions that setup the NIC struct and handle the sending and receiving of packets.
impl E1000Nic {
    /// Initializes the new E1000 network interface card that is connected as the given PciDevice.
    ///
    /// `enable_interrupts` must be called after the NIC has been registered with the `net` subsystem.
    pub fn init(e1000_pci_dev: &PciDevice) -> Result<&'static IrqSafeMutex<E1000Nic>, &'static str> {
        use interrupts::IRQ_BASE_OFFSET;

        //debug!("e1000_nc bar_type: {0}, mem_base: {1}, io_base: {2}", e1000_nc.bar_type, e1000_nc.mem_base, e1000_nc.io_base);
        
        // Get interrupt number
        let interrupt_num = match e1000_pci_dev.pci_get_interrupt_info() {
            Ok((Some(irq), _pin)) => (irq + IRQ_BASE_OFFSET) as InterruptNumber,
            _ => return Err("e1000: PCI device had no interrupt number (IRQ vector)"),
        };
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
        
        // initialize the buffer pool
        init_rx_buf_pool(RX_BUFFER_POOL_SIZE, E1000_RX_BUFFER_SIZE_IN_BYTES, &RX_BUFFER_POOL)?;

        let (rx_descs, rx_buffers) = Self::rx_init(&mut mapped_registers, &mut rx_registers)?;
        let rxq = RxQueue {
            id: 0,
            regs: rx_registers,
            rx_descs,
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
            tx_descs,
            num_tx_descs: E1000_NUM_TX_DESC,
            tx_cur: 0,
            cpu_id: None,
        };

        let e1000_nic = E1000Nic {
            bar_type,
            mem_base,
            interrupt_num,
            mac_hardware: mac_addr_hardware,
            mac_spoofed: None,
            rx_queue: rxq,
            tx_queue: txq,
            regs: mapped_registers,
            mac_regs: mac_registers,
            deferred_task: None,
        };
        
        let nic_ref = E1000_NIC.call_once(|| IrqSafeMutex::new(e1000_nic));
        Ok(nic_ref)
    }
    
    /// Initializes the interrupt handler and enables interrupts for this E1000 NIC.
    ///
    /// The provided `interface` must be the network interface associated with this E1000 NIC.
    /// This interface will be polled in a deferred task upon an interrupt being triggered
    /// for a received packet.
    pub fn init_interrupts(
        &mut self,
        interface: Arc<net::NetworkInterface>,
    ) -> Result<(), &'static str> {
        self.enable_interrupts();
        let deferred_task = deferred_interrupt_tasks::register_interrupt_handler(
            self.interrupt_num,
            e1000_handler,
            poll_interface,
            interface,
            Some(format!("e1000_deferred_task_irq_{:#X}", self.interrupt_num)),
        )
        .map_err(|error| {
            error!("error registering e1000 handler: {:?}", error);
            "e1000 interrupt number was already in use! Sharing IRQs is currently unsupported."
        })?;
        self.deferred_task = Some(deferred_task);

        Ok(())
    }

    /// Allocates memory for the NIC and maps the E1000 Register struct to that memory area.
    /// Returns a reference to the E1000 Registers, tied to their backing `MappedPages`.
    /// 
    /// # Arguments
    /// * `device`: reference to the nic device
    /// * `mem_base`: the physical address where the NIC's memory starts.
    fn map_e1000_regs(
        _device: &PciDevice,
        mem_base: PhysicalAddress,
    ) -> Result<(
        BorrowedMappedPages<E1000Registers, Mutable>, 
        BorrowedMappedPages<E1000RxRegisters, Mutable>, 
        BorrowedMappedPages<E1000TxRegisters, Mutable>, 
        BorrowedMappedPages<E1000MacRegisters, Mutable>
    ), &'static str> {

        const GENERAL_REGISTERS_SIZE_BYTES: usize = 8192;
        const RX_REGISTERS_SIZE_BYTES: usize = 4096;
        const TX_REGISTERS_SIZE_BYTES: usize = 4096;
        const MAC_REGISTERS_SIZE_BYTES: usize = 114_688;

        let mut physical_addr = mem_base;

        let nic_regs_mapped_page = map_frame_range(physical_addr, GENERAL_REGISTERS_SIZE_BYTES, MMIO_FLAGS)?;
        let regs = nic_regs_mapped_page.into_borrowed_mut(0).map_err(|(_mp, err)| err)?;
        physical_addr += GENERAL_REGISTERS_SIZE_BYTES;

        let nic_rx_regs_mapped_page = map_frame_range(physical_addr, RX_REGISTERS_SIZE_BYTES, MMIO_FLAGS)?;
        let rx_regs = nic_rx_regs_mapped_page.into_borrowed_mut(0).map_err(|(_mp, err)| err)?;
        physical_addr += RX_REGISTERS_SIZE_BYTES;

        let nic_tx_regs_mapped_page = map_frame_range(physical_addr, TX_REGISTERS_SIZE_BYTES, MMIO_FLAGS)?;
        let tx_regs = nic_tx_regs_mapped_page.into_borrowed_mut(0).map_err(|(_mp, err)| err)?;
        physical_addr += TX_REGISTERS_SIZE_BYTES;

        let nic_mac_regs_mapped_page = map_frame_range(physical_addr, MAC_REGISTERS_SIZE_BYTES, MMIO_FLAGS)?;
        let mac_regs = nic_mac_regs_mapped_page.into_borrowed_mut(0).map_err(|(_mp, err)| err)?;

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
        BorrowedSliceMappedPages<LegacyRxDescriptor, Mutable>, 
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
    ) -> Result<BorrowedSliceMappedPages<LegacyTxDescriptor, Mutable>, &'static str> {
        // get the queue of tx descriptors     
        let tx_descs = init_tx_queue(E1000_NUM_TX_DESC as usize, tx_regs)?;
        regs.tctl.write(regs::TCTL_EN | regs::TCTL_PSP);
        Ok(tx_descs)
    }

    /// Enable interrupts on this E1000 NIC.
    ///
    /// Currently this enables interrupts for:
    /// * Link Status Change
    /// * Receive transfers (incoming packets)
    fn enable_interrupts(&mut self) {
        //self.write_command(REG_IMASK ,0x1F6DC);
        //self.write_command(REG_IMASK ,0xff & !4);

        // Trigger interrupts on a Link Status Change and on a Receive Transfer.
        self.regs.ims.write(INT_LSC | INT_RX);
        // Clear all pending interrupts.
        self.regs.icr.read();
    }

    /// Clears pending interrupts by reading the Interrupt Control Register.
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
            self.rx_queue.poll_queue_and_store_received_packets()?;
            handled = true;
        }

        if !handled {
            error!("e1000::handle_interrupt(): unhandled interrupt!  status: {:#X}", status);
        } else if let Some(ref deferred_task) = self.deferred_task {
            let _ = deferred_task
                .unblock()
                .expect("BUG: e1000::handle_interrupt(): couldn't unblock deferred task");
        } else {
            error!("e1000::handle_interrupt(): no deferred task");
        }
        //regs.icr.read(); //clear interrupt
        Ok(())
    }
}

impl net::NetworkDevice for E1000Nic {
    fn send(&mut self, buf: TransmitBuffer) {
        self.tx_queue.send_on_queue(buf);
    }

    fn receive(&mut self) -> Option<ReceivedFrame> {
        self.rx_queue.received_frames.pop_front()
    }

    /// Returns the MAC address.
    fn mac_address(&self) -> [u8; 6] {
        self.mac_spoofed.unwrap_or(self.mac_hardware)
    }
}

extern "x86-interrupt" fn e1000_handler(_stack_frame: InterruptStackFrame) {
    if let Some(e1000_nic_ref) = E1000_NIC.get() {
        let mut e1000_nic = e1000_nic_ref.lock();
        if let Err(e) = e1000_nic.handle_interrupt() {
            error!("e1000_handler(): error handling interrupt: {:?}", e);
        }
        eoi(Some(e1000_nic.interrupt_num));
    } else {
        error!("BUG: e1000_handler(): E1000 NIC hasn't yet been initialized!");
    }
}

/// This function is used as a deferred interrupt task.
///
/// After processing the interrupt, the network interface associated with the `e1000` NIC will be
/// polled to process the received data.
///
/// Returns a result to comply with `deferred_interrupt_task::register_interrupt_handler`'s
/// signature.
fn poll_interface(interface: &Arc<net::NetworkInterface>) -> Result<(), ()> {
    interface.poll();
    Ok(())
}
