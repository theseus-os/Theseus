//! An ixgbe driver for a 82599 10GbE Network Interface Card.
//! Currently we support basic send and receive, Receive Side Scaling (RSS), 5-tuple filters, and MSI interrupts. 
//! We also support language-level virtualization of the NIC so that applications can directly access their assigned transmit and receive queues.
//! When using virtualization, we disable RSS since we use 5-tuple filters to ensure packets are routed to the correct queues.
//! We also disable interrupts, since we do not yet have support for allowing applications to register their own interrupt handlers.

#![no_std]
#![feature(untagged_unions)]
#![allow(dead_code)] //  to suppress warnings for unused functions/methods
#![allow(safe_packed_borrows)] // temporary, just to suppress unsafe packed borrows 
#![feature(abi_x86_interrupt)]

#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate static_assertions;
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
extern crate owning_ref;
extern crate rand;
extern crate hpet;
extern crate runqueue;
extern crate network_interface_card;
extern crate nic_initialization;
extern crate intel_ethernet;
extern crate nic_buffers;
extern crate nic_queues;
extern crate physical_nic;
extern crate virtual_nic;
extern crate zerocopy;
extern crate hashbrown;

mod regs;
mod queue_registers;
pub mod virtual_function;
pub mod test_packets;
use regs::*;
use queue_registers::*;

use spin::Once;
use alloc::{
    vec::Vec,
    collections::VecDeque,
    sync::Arc,
    boxed::Box
};
use irq_safety::MutexIrqSafe;
use memory::{PhysicalAddress, MappedPages};
use pci::{PciDevice, MSIX_CAPABILITY, PciConfigSpaceAccessMechanism};
use bit_field::BitField;
use interrupts::{eoi,register_msi_interrupt};
use x86_64::structures::idt::{ExceptionStackFrame, HandlerFunc};
use hpet::get_hpet;
use network_interface_card::NetworkInterfaceCard;
use nic_initialization::*;
use intel_ethernet::descriptors::{AdvancedRxDescriptor, AdvancedTxDescriptor};    
use nic_buffers::{TransmitBuffer, ReceiveBuffer, ReceivedFrame};
use nic_queues::{RxQueue, TxQueue};
use owning_ref::BoxRefMut;
use rand::{
    SeedableRng,
    RngCore,
    rngs::SmallRng
};
use core::mem::ManuallyDrop;
use hashbrown::HashMap;

/// Vendor ID for Intel
pub const INTEL_VEND:                   u16 = 0x8086;  

/// Device ID for the 82599ES, used to identify the device from the PCI space
pub const INTEL_82599:                  u16 = 0x10FB;  


/*** Default configuration at time of initialization of NIC ***/

/// If the ixgbe driver allows applications to request virtual NICs.
/// Currently, if this is true then interrupts and RSS need to be disabled because
/// these features should be enabled on a per-queue basis when the virtual NIC is created.
/// We do not support that currently.
const VIRTUALIZATION_ENABLED:               bool    = true;
/// If interrupts are enabled for packet reception
const INTERRUPT_ENABLE:                     bool    = false & !VIRTUALIZATION_ENABLED;
/// We do not access the PHY module for link information yet,
/// so this variable is set by the user depending on if the attached module is 10GB or 1GB.
const IXGBE_10GB_LINK:                      bool    = false;
/// If link uses 1GB SFP modules
const IXGBE_1GB_LINK:                       bool    = !(IXGBE_10GB_LINK);
/// The number of receive descriptors per queue
const IXGBE_NUM_RX_DESC:                    u16     = 512;
/// The number of transmit descriptors per queue
const IXGBE_NUM_TX_DESC:                    u16     = 512;
/// The number of receive queues that are enabled. 
/// I have tested till up to 16.
/// If RSS or filters are not enabled, this should be 1.
/// TODO: check all 128
const IXGBE_NUM_RX_QUEUES_ENABLED:          u8      = 16;
/// The maximum number of rx queues available on this NIC 
const IXGBE_MAX_RX_QUEUES:                  u8      = 128;
/// The number of transmit queues that are enabled. 
/// TODO: check all 128
const IXGBE_NUM_TX_QUEUES_ENABLED:          u8      = 16;
/// The maximum number of tx queues available on this NIC
const IXGBE_MAX_TX_QUEUES:                  u8      = 128;
/// Size of the Rx packet buffers
const IXGBE_RX_BUFFER_SIZE_IN_BYTES:        u16     = 8192;
/// The number of l34 5-tuple filters
const NUM_L34_5_TUPLE_FILTERS:              usize   = 128; 
/// If receive side scaling (where incoming packets are sent to different queues depending on a hash) is enabled.
const RSS_ENABLE:                           bool    = false & !VIRTUALIZATION_ENABLED;
/// Enable Direct Cache Access for the receive queues
/// TODO: Not working yet because we need to have a separate driver for DCA
/// which enables it for the CPU, chipset and registers devices that can use DCA (I think)
const DCA_ENABLE:                           bool    = false;
/// The number of MSI vectors enabled for the NIC, with the maximum for the 82599 being 64.
/// This number is only relevant if interrupts are enabled.
/// If we increase the number to >1, then we also have to add the interrupt handlers in
/// the NIC initialization function.
/// Currently we have only tested for 16 interrupts.
const NUM_MSI_VEC_ENABLED:                  u8      = 1; //IXGBE_NUM_RX_QUEUES_ENABLED;

/***********************************************************/


/// The single instance of the 82599 NIC.
pub static IXGBE_NIC: Once<MutexIrqSafe<IxgbeNic>> = Once::new();

/// Returns a reference to the IxgbeNic wrapped in a MutexIrqSafe,
/// if it exists and has been initialized.
pub fn get_ixgbe_nic() -> Option<&'static MutexIrqSafe<IxgbeNic>> {
    IXGBE_NIC.try()
}

/// How many ReceiveBuffers are preallocated for this driver to use. 
const RX_BUFFER_POOL_SIZE: usize = IXGBE_NUM_RX_QUEUES_ENABLED as usize * IXGBE_NUM_RX_DESC as usize * 2; 

lazy_static! {
    /// The pool of pre-allocated receive buffers that are used by the IXGBE NIC
    /// and temporarily given to higher layers in the networking stack.
    /// # Note
    /// The capacity always has to be greater than the number of buffers in the queue, which is why we multiply by 2.
    /// I'm not sure why that is with this implementation of an mpmc queue.
    static ref RX_BUFFER_POOL: mpmc::Queue<ReceiveBuffer> = mpmc::Queue::with_capacity(RX_BUFFER_POOL_SIZE * 2);
}

/// A struct representing an ixgbe network interface card
pub struct IxgbeNic {
    /// PCI information of this device
    pci_device: PciDevice,
    /// Type of Base Address Register 0,
    /// if it's memory mapped or I/O.
    bar_type: u8,
    /// MMIO Base Address     
    mem_base: PhysicalAddress,
    /// Hashmap to store the interrupt number for each msi vector.
    /// The key is the qid for the queue the interrupt is generated for,
    /// and the value is the interrupt number.
    interrupt_num: HashMap<u8,u8>,
    /// The actual MAC address burnt into the hardware  
    mac_hardware: [u8;6],       
    /// The optional spoofed MAC address to use in place of `mac_hardware` when transmitting.  
    mac_spoofed: Option<[u8; 6]>,
    /// Memory-mapped control registers
    regs1: BoxRefMut<MappedPages, IntelIxgbeRegisters1>,
    /// Memory-mapped control registers
    regs2: BoxRefMut<MappedPages, IntelIxgbeRegisters2>,
    /// Memory-mapped control registers
    regs3: BoxRefMut<MappedPages, IntelIxgbeRegisters3>,
    /// Memory-mapped control registers
    regs_mac: BoxRefMut<MappedPages, IntelIxgbeMacRegisters>,
    /// Memory-mapped msi-x vector table
    msix_vector_table: BoxRefMut<MappedPages, MsixVectorTable>,
    /// Array to store which L3/L4 5-tuple filters have been used.
    /// There are 128 such filters available.
    l34_5_tuple_filters: [bool; 128],
    /// The number of rx queues enabled
    num_rx_queues: u8,
    /// Vector of the enabled rx queues
    rx_queues: Vec<RxQueue<IxgbeRxQueueRegisters,AdvancedRxDescriptor>>,
    /// Registers for the disabled queues
    rx_registers_disabled: Vec<IxgbeRxQueueRegisters>,
    /// The number of tx queues enabled
    num_tx_queues: u8,
    /// Vector of the enabled tx queues
    tx_queues: Vec<TxQueue<IxgbeTxQueueRegisters,AdvancedTxDescriptor>>,
    /// Registers for the disabled queues
    tx_registers_disabled: Vec<IxgbeTxQueueRegisters>,
}

// A trait which contains common functionalities for a NIC
impl NetworkInterfaceCard for IxgbeNic {

    fn send_packet(&mut self, transmit_buffer: TransmitBuffer) -> Result<(), &'static str> {
        // by default, when using the physical NIC interface, we send on queue 0.
        let qid = 0;
        self.tx_queues[qid].send_on_queue(transmit_buffer);
        Ok(())
    }

    fn get_received_frame(&mut self) -> Option<ReceivedFrame> {
        // by default, when using the physical NIC interface, we receive on queue 0.
        let qid = 0;
        // return one frame from the queue's received frames
        self.rx_queues[qid].received_frames.pop_front()
    }

    fn poll_receive(&mut self) -> Result<(), &'static str> {
        // by default, when using the physical NIC interface, we receive on queue 0.
        let qid = 0;
        self.rx_queues[qid].remove_frames_from_queue()
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac_spoofed.unwrap_or(self.mac_hardware)
    }
}

// Functions that setup the NIC struct and handle the sending and receiving of packets
impl IxgbeNic {
    /// Store required values from the device's PCI config space,
    /// and initialize different features of the nic.
    pub fn init(ixgbe_pci_dev: &PciDevice) -> Result<&'static MutexIrqSafe<IxgbeNic>, &'static str> {
        let bar0 = ixgbe_pci_dev.bars[0];
        // Determine the type from the base address register
        let bar_type = (bar0 as u8) & 0x01;    

        // If the base address is not memory mapped then exit
        if bar_type == PciConfigSpaceAccessMechanism::IoPort as u8 {
            error!("ixgbe::init(): BAR0 is of I/O type");
            return Err("ixgbe::init(): BAR0 is of I/O type")
        }

        // 16-byte aligned memory mapped base address
        let mem_base =  ixgbe_pci_dev.determine_mem_base()?;

        // map the IntelIxgbeRegisters structs to the address found from the pci space
        let (mut mapped_registers1, mut mapped_registers2, mut mapped_registers3, mut mapped_registers_mac, 
            mut rx_mapped_registers, mut tx_mapped_registers) = Self::mapped_reg(mem_base)?;

        // map the msi-x vector table to an address found from the pci space
        let mut vector_table = Self::mem_map_msix(ixgbe_pci_dev)?;

        // link initialization
        Self::start_link(&mut mapped_registers1, &mut mapped_registers2, &mut mapped_registers3, &mut mapped_registers_mac)?;

        // clear stats registers
        Self::clear_stats(&mapped_registers2);

        // store the mac address of this device
        let mac_addr_hardware = Self::read_mac_address_from_nic(&mut mapped_registers_mac);

        // initialize the buffer pool
        init_rx_buf_pool(RX_BUFFER_POOL_SIZE, IXGBE_RX_BUFFER_SIZE_IN_BYTES, &RX_BUFFER_POOL)?;

        // create the rx desc queues and their packet buffers
        let (mut rx_descs, mut rx_buffers) = Self::rx_init(&mut mapped_registers1, &mut mapped_registers2, &mut rx_mapped_registers)?;
        
        // create the vec of rx queues
        let mut rx_queues = Vec::new();
        let mut id = 0;
        while !rx_descs.is_empty() {
            let rx_queue = RxQueue {
                id: id,
                regs: rx_mapped_registers.remove(0),
                rx_descs: rx_descs.remove(0),
                num_rx_descs: IXGBE_NUM_RX_DESC,
                rx_cur: 0,
                rx_bufs_in_use: rx_buffers.remove(0),  
                rx_buffer_size_bytes: IXGBE_RX_BUFFER_SIZE_IN_BYTES,
                received_frames: VecDeque::new(),
                cpu_id : None,
                rx_buffer_pool: &RX_BUFFER_POOL,
                filter_num: None
            };
            rx_queues.push(rx_queue);
            id += 1;
        }


        // create the tx descriptor queues
        let mut tx_descs = Self::tx_init(&mut mapped_registers2, &mut mapped_registers_mac, &mut tx_mapped_registers)?;
        
        // create the vec of tx queues
        let mut tx_queues = Vec::new();
        let mut id = 0;
        while !tx_descs.is_empty() {
            let tx_queue = TxQueue {
                id: id,
                regs: tx_mapped_registers.remove(0),
                tx_descs: tx_descs.remove(0),
                num_tx_descs: IXGBE_NUM_TX_DESC,
                tx_cur: 0,
                cpu_id : None,
            };
            tx_queues.push(tx_queue);
            id += 1;
        }

        // enable msi-x interrupts if required and return the assigned interrupt numbers
        let interrupt_num =
            if INTERRUPT_ENABLE {
                // the collection of interrupt handlers for the receive queues. There might be a better way to do this 
                // but right now we have to explicitly create each interrupt handler and add it here.
                let interrupt_handlers: [HandlerFunc; NUM_MSI_VEC_ENABLED as usize] = [ixgbe_handler_0];
                ixgbe_pci_dev.pci_enable_msix()?;
                ixgbe_pci_dev.pci_set_interrupt_disable_bit();
                Self::enable_msix_interrupts(&mut mapped_registers1, &mut rx_queues, &mut vector_table, &interrupt_handlers)?
            }
            else {
                HashMap::new()
            };

        // enable Receive Side Scaling if required
        if RSS_ENABLE {
            Self::enable_rss(&mut mapped_registers2, &mut mapped_registers3)?;
        }

        if DCA_ENABLE {
            Self::enable_dca(&mut mapped_registers3, &mut rx_queues)?;
        }

        Self::wait_for_link(&mapped_registers2);

        let ixgbe_nic = IxgbeNic {
            pci_device: ixgbe_pci_dev.clone(),
            bar_type: bar_type,
            mem_base: mem_base,
            interrupt_num: interrupt_num,
            mac_hardware: mac_addr_hardware,
            mac_spoofed: None,
            regs1: mapped_registers1,
            regs2: mapped_registers2,
            regs3: mapped_registers3,
            regs_mac: mapped_registers_mac,
            msix_vector_table: vector_table,
            l34_5_tuple_filters: [false; NUM_L34_5_TUPLE_FILTERS],
            num_rx_queues: IXGBE_NUM_RX_QUEUES_ENABLED,
            rx_queues: rx_queues,
            rx_registers_disabled: rx_mapped_registers,
            num_tx_queues: IXGBE_NUM_TX_QUEUES_ENABLED,
            tx_queues: tx_queues,
            tx_registers_disabled: tx_mapped_registers,
        };

        info!("Link is up with speed: {} Mb/s", ixgbe_nic.link_speed());

        let nic_ref = IXGBE_NIC.call_once(|| MutexIrqSafe::new(ixgbe_nic));
        Ok(nic_ref)
    }

    /// Returns the memory-mapped control registers of the nic and the rx/tx queue registers.
    fn mapped_reg(mem_base: PhysicalAddress) 
        -> Result<(BoxRefMut<MappedPages, IntelIxgbeRegisters1>, 
                    BoxRefMut<MappedPages, IntelIxgbeRegisters2>, 
                    BoxRefMut<MappedPages, IntelIxgbeRegisters3>, 
                    BoxRefMut<MappedPages, IntelIxgbeMacRegisters>, 
                    Vec<IxgbeRxQueueRegisters>, Vec<IxgbeTxQueueRegisters>), &'static str> 
    {
        // We've divided the memory-mapped registers into multiple regions.
        // The size of each region is found from the data sheet, but it always lies on a page boundary.
        const GENERAL_REGISTERS_1_SIZE_BYTES:   usize = 4096;
        const RX_REGISTERS_SIZE_BYTES:          usize = 4096;
        const GENERAL_REGISTERS_2_SIZE_BYTES:   usize = 4 * 4096;
        const TX_REGISTERS_SIZE_BYTES:          usize = 2 * 4096;
        const MAC_REGISTERS_SIZE_BYTES:         usize = 5 * 4096;
        const GENERAL_REGISTERS_3_SIZE_BYTES:   usize = 18 * 4096;

        // Allocate memory for the registers, making sure each successive memory region begins where the previous region ended.
        let nic_regs1_mapped_page = allocate_memory(mem_base, GENERAL_REGISTERS_1_SIZE_BYTES)?;
        let nic_rx_regs1_mapped_page = allocate_memory(mem_base + GENERAL_REGISTERS_1_SIZE_BYTES, RX_REGISTERS_SIZE_BYTES)?;
        let nic_regs2_mapped_page = allocate_memory(mem_base + GENERAL_REGISTERS_1_SIZE_BYTES + RX_REGISTERS_SIZE_BYTES, GENERAL_REGISTERS_2_SIZE_BYTES)?;        
        let nic_tx_regs_mapped_page = allocate_memory(mem_base + GENERAL_REGISTERS_1_SIZE_BYTES + RX_REGISTERS_SIZE_BYTES + GENERAL_REGISTERS_2_SIZE_BYTES, TX_REGISTERS_SIZE_BYTES)?;
        let nic_mac_regs_mapped_page = allocate_memory(mem_base + GENERAL_REGISTERS_1_SIZE_BYTES + RX_REGISTERS_SIZE_BYTES + GENERAL_REGISTERS_2_SIZE_BYTES + TX_REGISTERS_SIZE_BYTES, MAC_REGISTERS_SIZE_BYTES)?;
        let nic_rx_regs2_mapped_page = allocate_memory(mem_base + GENERAL_REGISTERS_1_SIZE_BYTES + RX_REGISTERS_SIZE_BYTES + GENERAL_REGISTERS_2_SIZE_BYTES + TX_REGISTERS_SIZE_BYTES + MAC_REGISTERS_SIZE_BYTES, RX_REGISTERS_SIZE_BYTES)?;        
        let nic_regs3_mapped_page = allocate_memory(mem_base + GENERAL_REGISTERS_1_SIZE_BYTES + RX_REGISTERS_SIZE_BYTES + GENERAL_REGISTERS_2_SIZE_BYTES + TX_REGISTERS_SIZE_BYTES + MAC_REGISTERS_SIZE_BYTES + RX_REGISTERS_SIZE_BYTES, GENERAL_REGISTERS_3_SIZE_BYTES)?;

        // Map the memory as the register struct and tie the lifetime of the struct with its backing mapped pages
        let regs1 = BoxRefMut::new(Box::new(nic_regs1_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<IntelIxgbeRegisters1>(0))?;
        let regs2 = BoxRefMut::new(Box::new(nic_regs2_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<IntelIxgbeRegisters2>(0))?;
        let regs3 = BoxRefMut::new(Box::new(nic_regs3_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<IntelIxgbeRegisters3>(0))?;
        let mac_regs = BoxRefMut::new(Box::new(nic_mac_regs_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<IntelIxgbeMacRegisters>(0))?;
        
        // Divide the pages of the Rx queue registers into multiple 64B regions
        let mut regs_rx = Self::mapped_regs_from_rx_memory(nic_rx_regs1_mapped_page);
        regs_rx.append(&mut Self::mapped_regs_from_rx_memory(nic_rx_regs2_mapped_page));
        
        // Divide the pages of the Tx queue registers into multiple 64B regions
        let regs_tx = Self::mapped_regs_from_tx_memory(nic_tx_regs_mapped_page);
            
        Ok((regs1, regs2, regs3, mac_regs, regs_rx, regs_tx))
    }

    /// Split the pages where rx queue registers are mapped into multiple smaller memory regions.
    /// One region contains all the registers for a single queue.
    fn mapped_regs_from_rx_memory(mp: MappedPages) -> Vec<IxgbeRxQueueRegisters> {
        const QUEUES_IN_MP: usize = 64;
        const RX_QUEUE_REGISTERS_SIZE_BYTES: usize = core::mem::size_of::<RegistersRx>();
        
        assert!(mp.size_in_bytes() >= QUEUES_IN_MP * RX_QUEUE_REGISTERS_SIZE_BYTES);

        let starting_address = mp.start_address();

        // We share the backing mapped pages among all the queue registers
        let shared_mp = Arc::new(mp);
        let mut pointers_to_queues = Vec::with_capacity(QUEUES_IN_MP);

        for i in 0..QUEUES_IN_MP {
            // This is safe because we have checked that the number of queues we want to partition from these mapped pages fit into the allocated memory,
            // and that each queue starts at the end of the previous.
            // We also ensure that the backing mapped pages are included in the same struct as the registers, almost as a pseudo OwningRef
            let registers = unsafe{ Box::from_raw((starting_address.value() + (i * RX_QUEUE_REGISTERS_SIZE_BYTES)) as *mut RegistersRx) };
            pointers_to_queues.push(
                IxgbeRxQueueRegisters {
                    regs: ManuallyDrop::new(registers),
                    backing_pages: shared_mp.clone()
                }
            );
        }
        pointers_to_queues
    }

    /// Split the pages where tx queue registers are mapped into multiple smaller memory regions.
    /// One region contains all the registers for a single queue.
    fn mapped_regs_from_tx_memory(mp: MappedPages) -> Vec<IxgbeTxQueueRegisters> {
        const QUEUES_IN_MP: usize = 128;
        const TX_QUEUE_REGISTERS_SIZE_BYTES: usize = core::mem::size_of::<RegistersTx>();
        
        assert!(mp.size_in_bytes() >= QUEUES_IN_MP * TX_QUEUE_REGISTERS_SIZE_BYTES);

        let starting_address = mp.start_address();

        // We share the backing mapped pages among all the queue registers
        let shared_mp = Arc::new(mp);
        let mut pointers_to_queues = Vec::with_capacity(QUEUES_IN_MP);

        for i in 0..QUEUES_IN_MP {
            // This is safe because we have checked that the number of queues we want to partition from these mapped pages fit into the allocated memory,
            // and that each queue starts at the end of the previous.
            // We also ensure that the backing mapped pages are included in the same struct as the registers, almost as a pseudo OwningRef
            let registers = unsafe{ Box::from_raw((starting_address.value() + (i * TX_QUEUE_REGISTERS_SIZE_BYTES)) as *mut RegistersTx) };
            pointers_to_queues.push(
                IxgbeTxQueueRegisters {
                    regs: ManuallyDrop::new(registers),
                    backing_pages: shared_mp.clone()
                }
            );
        }
        pointers_to_queues
    }

    /// Returns the memory mapped msix vector table
    pub fn mem_map_msix(dev: &PciDevice) -> Result<BoxRefMut<MappedPages, MsixVectorTable>, &'static str> {
        // retreive the address in the pci config space for the msi-x capability
        let cap_addr = dev.find_pci_capability(MSIX_CAPABILITY).ok_or("ixgbe: device does not have MSI-X capability")?;
        // find the BAR used for msi-x
        let vector_table_offset = 4;
        let table_offset = dev.pci_read_32(cap_addr + vector_table_offset);
        let bar = table_offset & 0x7;
        let offset = table_offset >> 3;
        // find the memory base address and size of the area for the vector table
        let mem_base = PhysicalAddress::new((dev.bars[bar as usize] + offset) as usize)?;
        let mem_size_in_bytes = core::mem::size_of::<MsixVectorEntry>() * IXGBE_MAX_MSIX_VECTORS;

        // debug!("msi-x vector table bar: {}, base_address: {:#X} and size: {} bytes", bar, mem_base, mem_size_in_bytes);

        let msix_mapped_pages = allocate_memory(mem_base, mem_size_in_bytes)?;
        let vector_table = BoxRefMut::new(Box::new(msix_mapped_pages)).try_map_mut(|mp| mp.as_type_mut::<MsixVectorTable>(0))?;

        Ok(vector_table)
    }

    pub fn spoof_mac(&mut self, spoofed_mac_addr: [u8; 6]) {
        self.mac_spoofed = Some(spoofed_mac_addr);
    }

    /// Reads the actual MAC address burned into the NIC hardware.
    fn read_mac_address_from_nic(regs: &IntelIxgbeMacRegisters) -> [u8; 6] {
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

    /// Acquires semaphore to synchronize between software and firmware (10.5.4)
    fn acquire_semaphore(regs: &mut IntelIxgbeRegisters3) -> Result<bool, &'static str> {
        // femtoseconds per millisecond
        const FS_PER_MS: u64 = 1_000_000_000_000;
        let hpet = get_hpet();
        let hpet_ref = hpet.as_ref().ok_or("ixgbe::acquire_semaphore: couldn't get HPET timer")?;
        let period_fs: u64 = hpet_ref.counter_period_femtoseconds() as u64;        

        // check that some other sofware is not using the semaphore
        // 1. poll SWSM.SMBI bit until reads as 0 or 10ms timer expires
        let start = hpet_ref.get_counter();
        let mut timer_expired_smbi = false;
        let mut smbi_bit = 1;
        while smbi_bit != 0 {
            smbi_bit = regs.swsm.read() & SWSM_SMBI;
            let end = hpet_ref.get_counter();

            let expiration_time = 10;
            if (end-start) * period_fs / FS_PER_MS == expiration_time {
                timer_expired_smbi = true;
                break;
            }
        } 
        // now, hardware will auto write 1 to the SMBI bit

        // check that firmware is not using the semaphore
        // 1. write to SWESMBI bit
        let set_swesmbi = regs.swsm.read() | SWSM_SWESMBI; // set bit 1 to 1
        regs.swsm.write(set_swesmbi);

        // 2. poll SWSM.SWESMBI bit until reads as 1 or 3s timer expires
        let start = hpet_ref.get_counter();
        let mut swesmbi_bit = 0;
        let mut timer_expired_swesmbi = false;
        while swesmbi_bit == 0 {
            swesmbi_bit = (regs.swsm.read() & SWSM_SWESMBI) >> 1;
            let end = hpet_ref.get_counter();

            let expiration_time = 3000;
            if (end-start) * period_fs / FS_PER_MS == expiration_time {
                timer_expired_swesmbi = true;
                break;
            }
        } 

        // software takes control of the requested resource
        // 1. read firmware and software bits of sw_fw_sync register 
        let mut sw_fw_sync_smbits = regs.sw_fw_sync.read() & SW_FW_SYNC_SMBITS_MASK;
        let sw_sync_shift = 3;
        let fw_sync_shift = 8;
        let sw_mac = (sw_fw_sync_smbits & SW_FW_SYNC_SW_MAC) >> sw_sync_shift;
        let fw_mac = (sw_fw_sync_smbits & SW_FW_SYNC_FW_MAC) >> fw_sync_shift;

        // clear sw sempahore bits if sw malfunctioned
        if timer_expired_smbi {
            sw_fw_sync_smbits &= !(SW_FW_SYNC_SMBITS_SW);
        }

        // clear fw semaphore bits if fw malfunctioned
        if timer_expired_swesmbi {
            sw_fw_sync_smbits &= !(SW_FW_SYNC_SMBITS_FW);
        }

        regs.sw_fw_sync.write(sw_fw_sync_smbits);

        // check if semaphore bits for the resource are cleared
        // then resources are available
        if (sw_mac == 0) && (fw_mac == 0) {
            //claim the sw resource by setting the bit
            let sw_fw_sync = regs.sw_fw_sync.read() & SW_FW_SYNC_SW_MAC;
            regs.sw_fw_sync.write(sw_fw_sync);

            //clear bits in the swsm register
            let swsm = regs.swsm.read() & !(SWSM_SMBI) & !(SWSM_SWESMBI);
            regs.swsm.write(swsm);

            return Ok(true);
        }

        //resource is not available
        else {
            //clear bits in the swsm register
            let swsm = regs.swsm.read() & !(SWSM_SMBI) & !(SWSM_SWESMBI);
            regs.swsm.write(swsm);

            Ok(false)
        }
    }

    /// Release the semaphore synchronizing between software and firmware
    fn release_semaphore(regs: &mut IntelIxgbeRegisters3) {
        // clear bit of released resource
        let sw_fw_sync = regs.sw_fw_sync.read() & !(SW_FW_SYNC_SW_MAC);
        regs.sw_fw_sync.write(sw_fw_sync);

        // release semaphore
        let _swsm = regs.swsm.read() & !(SWSM_SMBI) & !(SWSM_SWESMBI);
    }

    /// Software reset of NIC to get it running
    fn start_link (regs1: &mut IntelIxgbeRegisters1, regs2: &mut IntelIxgbeRegisters2, regs3: &mut IntelIxgbeRegisters3, regs_mac: &mut IntelIxgbeMacRegisters) 
        -> Result<(), &'static str>
    {
        //disable interrupts: write to EIMC registers, 1 in b30-b0, b31 is reserved
        regs1.eimc.write(DISABLE_INTERRUPTS);

        // master disable algorithm (sec 5.2.5.3.2)
        // global reset = sw reset + link reset 
        let val = regs1.ctrl.read();
        regs1.ctrl.write(val|CTRL_RST|CTRL_LRST);

        //wait 10 ms
        let wait_time = 10_000;
        let _ =pit_clock::pit_wait(wait_time);

        //disable flow control.. write 0 TO FCTTV, FCRTL, FCRTH, FCRTV and FCCFG
        for fcttv in regs2.fcttv.iter_mut() {
            fcttv.write(0);
        }

        for fcrtl in regs2.fcrtl.iter_mut() {
            fcrtl.write(0);
        }

        for fcrth in regs2.fcrth.iter_mut() {
            fcrth.write(0);
        }

        regs2.fcrtv.write(0);
        regs2.fccfg.write(0);

        //disable interrupts
        regs1.eims.write(DISABLE_INTERRUPTS);

        //wait for eeprom auto read completion
        while !regs3.eec.read().get_bit(EEC_AUTO_RD as u8){}

        //read MAC address
        debug!("Ixgbe: MAC address low: {:#X}", regs_mac.ral.read());
        debug!("Ixgbe: MAC address high: {:#X}", regs_mac.rah.read() & 0xFFFF);

        //wait for dma initialization done (RDRXCTL.DMAIDONE)
        let mut val = regs2.rdrxctl.read();
        let dmaidone_bit = 1 << 3;
        while val & dmaidone_bit != dmaidone_bit {
            val = regs2.rdrxctl.read();
        }

        while Self::acquire_semaphore(regs3)? {
            //wait 10 ms
            let _ =pit_clock::pit_wait(wait_time);
        }

        //setup PHY and the link 
        if IXGBE_10GB_LINK {
            let val = regs2.autoc.read() & !(AUTOC_LMS_CLEAR);
            regs2.autoc.write(val | AUTOC_LMS_10_GBE_S); // value should be 0xC09C_6004

            let val = regs2.autoc.read() & !(AUTOC_10G_PMA_PMD_CLEAR);
            regs2.autoc.write(val | AUTOC_10G_PMA_PMD_XAUI); // value should be 0xC09C_6004

            // let val = regs2.autoc2.read() & !(AUTOC2_10G_PMA_PMD_S_CLEAR);
            // regs2.autoc2.write(val | AUTOC2_10G_PMA_PMD_S_SFI); // value should be 0xA_0000
        }
        else {
            let mut val = regs2.autoc.read();
            val = (val & !(AUTOC_LMS_1_GB) & !(AUTOC_1G_PMA_PMD)) | AUTOC_FLU;
            regs2.autoc.write(val);
        }

        let val = regs2.autoc.read();
        regs2.autoc.write(val|AUTOC_RESTART_AN); 

        Self::release_semaphore(regs3);        

        // debug!("STATUS: {:#X}", regs.status.read()); 
        // debug!("CTRL: {:#X}", regs.ctrl.read());
        // debug!("LINKS: {:#X}", regs.links.read()); //b7 and b30 should be 1 for link up 
        // debug!("AUTOC: {:#X}", regs.autoc.read()); 
        // debug!("AUTOC2: {:#X}", regs.autoc2.read()); 

        Ok(())
    }

    /// Returns value of (links, links2) registers
    pub fn link_status(&self) -> (u32, u32) {
        (self.regs2.links.read(), self.regs2.links2.read())
    }

    /// Returns link speed in Mb/s
    pub fn link_speed(&self) -> u32 {
        let speed = self.regs2.links.read() & LINKS_SPEED_MASK; 
        const LS_100: u32 = 0x1 << 28;
        const LS_1000: u32 = 0x2 << 28;
        const LS_10000: u32 = 0x3 << 28;

        if speed == LS_100 {
            100
        } else if speed == LS_1000 {
            1000
        } else if speed == LS_10000 {
            10_000
        } else {
            0
        }
    }

    /// Wait for link to be up for upto 10 seconds
    fn wait_for_link(regs2: &IntelIxgbeRegisters2) {
        // wait 10 ms between tries
        let wait_time = 10_000;
        // wait for a total of 10 s
        let total_tries = 10_000_000 / wait_time;
        let mut tries = 0;

        while (regs2.links.read() & LINKS_SPEED_MASK == 0) && (tries < total_tries) {
            let _ = pit_clock::pit_wait(wait_time);
            tries += 1;
        }
    }

    /// Clear the statistic registers by reading from them
    fn clear_stats(regs: &IntelIxgbeRegisters2) {
        regs.gprc.read();
        regs.gptc.read();
        regs.gorcl.read();
        regs.gorch.read();
        regs.gotcl.read();
        regs.gotch.read();
    }

    /// Returns the Rx and Tx statistics in the form:  (Good Rx packets, Good Rx bytes, Good Tx packets, Good Tx bytes).
    /// A good packet is one that is >= 64 bytes including ethernet header and CRC
    pub fn get_stats(&self) -> (u32,u64,u32,u64){
        let rx_bytes =  ((self.regs2.gorch.read() as u64 & 0xF) << 32) | self.regs2.gorcl.read() as u64;
        let tx_bytes =  ((self.regs2.gotch.read() as u64 & 0xF) << 32) | self.regs2.gotcl.read() as u64;

        (self.regs2.gprc.read(), rx_bytes, self.regs2.gptc.read(), tx_bytes)
    }

    /// Initializes the array of receive descriptors and their corresponding receive buffers,
    /// and returns a tuple including both of them for all rx queues in use.
    /// Also enables receive functionality for the NIC.
    fn rx_init(regs1: &mut IntelIxgbeRegisters1, regs: &mut IntelIxgbeRegisters2, rx_regs: &mut Vec<IxgbeRxQueueRegisters>) 
        -> Result<(Vec<BoxRefMut<MappedPages, [AdvancedRxDescriptor]>>, Vec<Vec<ReceiveBuffer>>), &'static str>  
    {
        let mut rx_descs_all_queues = Vec::new();
        let mut rx_bufs_in_use_all_queues = Vec::new();

        Self::disable_rx_function(regs);
        // program RXPBSIZE according to DCB and virtualization modes (both off)
        regs.rxpbsize[0].write(RXPBSIZE_512KB);
        for i in 1..8 {
            regs.rxpbsize[i].write(0);
        }
        //CRC offloading
        regs.hlreg0.write(regs.hlreg0.read() | HLREG0_CRC_STRIP);
        regs.rdrxctl.write(regs.rdrxctl.read() | RDRXCTL_CRC_STRIP);
        // Clear bits
        regs.rdrxctl.write(regs.rdrxctl.read() & !RDRXCTL_RSCFRSTSIZE);

        for qid in 0..IXGBE_NUM_RX_QUEUES_ENABLED {
            let rxq = &mut rx_regs[qid as usize];        

            // get the queue of rx descriptors and their corresponding rx buffers
            let (rx_descs, rx_bufs_in_use) = init_rx_queue(IXGBE_NUM_RX_DESC as usize, &RX_BUFFER_POOL, IXGBE_RX_BUFFER_SIZE_IN_BYTES as usize, rxq)?;          
            
            //set the size of the packet buffers and the descriptor format used
            let mut val = rxq.srrctl.read();
            val.set_bits(0..4, BSIZEPACKET_8K);
            val.set_bits(8..13, BSIZEHEADER_0B);
            val.set_bits(25..27, DESCTYPE_ADV_1BUFFER);
            val = val | DROP_ENABLE;
            rxq.srrctl.write(val);

            //enable the rx queue
            let val = rxq.rxdctl.read();
            rxq.rxdctl.write(val | RX_Q_ENABLE);

            //make sure queue is enabled
            while rxq.rxdctl.read() & RX_Q_ENABLE == 0 {}
        
            // set bit 12 to 0
            let val = rxq.dca_rxctrl.read();
            rxq.dca_rxctrl.write(val & !DCA_RXCTRL_CLEAR_BIT_12);

            // Write the tail index.
            // Note that the 82599 datasheet (section 8.2.3.8.5) states that we should set the RDT (tail index) to the index *beyond* the last receive descriptor, 
            // but we set it to the last receive descriptor for the same reason as the e1000 driver
            rxq.rdt.write((IXGBE_NUM_RX_DESC - 1) as u32);
            
            rx_descs_all_queues.push(rx_descs);
            rx_bufs_in_use_all_queues.push(rx_bufs_in_use);
        }
        
        Self::enable_rx_function(regs1,regs);
        Ok((rx_descs_all_queues, rx_bufs_in_use_all_queues))
    }

    /// disable receive functionality
    fn disable_rx_function(regs: &mut IntelIxgbeRegisters2) {        
        let val = regs.rxctrl.read();
        regs.rxctrl.write(val & !RECEIVE_ENABLE); 
    }

    /// enable receive functionality
    fn enable_rx_function(regs1: &mut IntelIxgbeRegisters1,regs: &mut IntelIxgbeRegisters2) {
        // set rx parameters of which type of packets are accepted by the nic
        // right now we allow the nic to receive all types of packets, even incorrectly formed ones
        regs.fctrl.write(STORE_BAD_PACKETS | MULTICAST_PROMISCUOUS_ENABLE | UNICAST_PROMISCUOUS_ENABLE | BROADCAST_ACCEPT_MODE); 
        
        // some magic numbers
        regs1.ctrl_ext.write(regs1.ctrl_ext.read() | CTRL_EXT_NO_SNOOP_DIS);

        // enable receive functionality
        let val = regs.rxctrl.read();
        regs.rxctrl.write(val | RECEIVE_ENABLE); 
    }

    /// Initialize the array of transmit descriptors for all queues and returns them.
    /// Also enables transmit functionality for the NIC.
    fn tx_init(regs: &mut IntelIxgbeRegisters2, regs_mac: &mut IntelIxgbeMacRegisters, tx_regs: &mut Vec<IxgbeTxQueueRegisters>) 
        -> Result<Vec<BoxRefMut<MappedPages, [AdvancedTxDescriptor]>>, &'static str>   
    {
        // disable transmission
        Self::disable_transmission(regs);

        // CRC offload and small packet padding enable
        regs.hlreg0.write(regs.hlreg0.read() | HLREG0_TXCRCEN | HLREG0_TXPADEN);

        // Set RTTFCS.ARBDIS to 1
        regs.rttdcs.write(regs.rttdcs.read() | RTTDCS_ARBDIS);

        // program DTXMXSZRQ and TXPBSIZE according to DCB and virtualization modes (both off)
        regs_mac.txpbsize[0].write(TXPBSIZE_160KB);
        for i in 1..8 {
            regs_mac.txpbsize[i].write(0);
        }
        regs_mac.dtxmxszrq.write(DTXMXSZRQ_MAX_BYTES); 

        // Clear RTTFCS.ARBDIS
        regs.rttdcs.write(regs.rttdcs.read() & !RTTDCS_ARBDIS);

        let mut tx_descs_all_queues = Vec::new();
        
        for qid in 0..IXGBE_NUM_TX_QUEUES_ENABLED {
            let txq = &mut tx_regs[qid as usize];

            let tx_descs = init_tx_queue(IXGBE_NUM_TX_DESC as usize, txq)?;
        
            if qid == 0 {
                // enable transmit operation, only have to do this for the first queue
                Self::enable_transmission(regs);
            }

            // Set descriptor thresholds
            // If we enable this then we need to change the packet send function to stop polling
            // for a descriptor done on every packet sent
            // txq.txdctl.write(TXDCTL_PTHRESH | TXDCTL_HTHRESH | TXDCTL_WTHRESH); 

            //enable tx queue
            let val = txq.txdctl.read();
            txq.txdctl.write(val | TX_Q_ENABLE); 

            //make sure queue is enabled
            while txq.txdctl.read() & TX_Q_ENABLE == 0 {} 

            tx_descs_all_queues.push(tx_descs);
        }
        Ok(tx_descs_all_queues)
    }  

    /// disable transmit functionality
    fn disable_transmission(regs: &mut IntelIxgbeRegisters2) {
        let val = regs.dmatxctl.read();
        regs.dmatxctl.write(val & !TE); 
    }

    /// enable transmit functionality
    fn enable_transmission(regs: &mut IntelIxgbeRegisters2) {
        let val = regs.dmatxctl.read();
        regs.dmatxctl.write(val | TE); 
    }

    /// Enable multiple receive queues with RSS.
    /// Part of queue initialization is done in the rx_init function.
    pub fn enable_rss(regs2: &mut IntelIxgbeRegisters2, regs3: &mut IntelIxgbeRegisters3) -> Result<(), &'static str>{
        // enable RSS writeback in the header field of the receive descriptor
        regs2.rxcsum.write(RXCSUM_PCSD);
        
        // enable RSS and set fields that will be used by hash function
        // right now we're using the udp port and ipv4 address.
        regs3.mrqc.write(MRQC_MRQE_RSS | MRQC_UDPIPV4 ); 

        //set the random keys for the hash function
        let seed = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
        let mut rng = SmallRng::seed_from_u64(seed);
        for rssrk in regs3.rssrk.iter_mut() {
            rssrk.write(rng.next_u32());
        }

        // Initialize the RSS redirection table
        // each reta register has 4 redirection entries
        // since mapping to queues is random and based on a hash, we randomly assign 1 queue to each reta register
        let mut qid = 0;
        for reta in regs3.reta.iter_mut() {
            //set 4 entries to the same queue number
            let val = qid << RETA_ENTRY_0_OFFSET | qid << RETA_ENTRY_1_OFFSET | qid << RETA_ENTRY_2_OFFSET | qid << RETA_ENTRY_3_OFFSET;
            reta.write(val);

            // next 4 entries will be assigned to the next queue
            qid = (qid + 1) % IXGBE_NUM_RX_QUEUES_ENABLED as u32;
        }

        Ok(())
    }

    /// Enables Direct Cache Access for the device.
    fn enable_dca(regs: &mut IntelIxgbeRegisters3, rxq: &mut Vec<RxQueue<IxgbeRxQueueRegisters,AdvancedRxDescriptor>>) -> Result<(), &'static str> {
        // Enable DCA tagging, which writes the cpu id to the PCIe Transaction Layer Packets (TLP)
        // There are 2 version of DCA that are mentioned, legacy and 1.0
        // We always enable 1.0 since (1) currently haven't found additional info online and (2) the linux driver always enables 1.0 
        regs.dca_ctrl.write(DCA_MODE_2 | DCA_CTRL_ENABLE);
        Self::enable_rx_dca(rxq)  
    }

    /// Sets up DCA for the rx queues that have been enabled.
    /// You can optionally choose to have the descriptor, header and payload copied to the cache for each received packet
    fn enable_rx_dca(rx_queues: &mut Vec<RxQueue<IxgbeRxQueueRegisters,AdvancedRxDescriptor>>) -> Result<(), &'static str>{

        for rxq in rx_queues {            
            // the cpu id will tell which cache the data will need to be written to
            // TODO: choose a better default value
            let cpu_id = rxq.cpu_id.unwrap_or(0) as u32;
            
            // currently allowing only write of rx descriptors to cache since packet payloads are very large
            // A note from linux ixgbe driver: 
            // " We can enable relaxed ordering for reads, but not writes when
            //   DCA is enabled.  This is due to a known issue in some chipsets
            //   which will cause the DCA tag to be cleared."
            rxq.regs.dca_rxctrl.write(RX_DESC_DCA_ENABLE | RX_HEADER_DCA_ENABLE | RX_PAYLOAD_DCA_ENABLE | RX_DESC_R_RELAX_ORDER_EN | (cpu_id << DCA_CPUID_SHIFT));
        }
        Ok(())
    }

    /// Sets the L3/L4 5-tuple filter which can do an exact match of the packet's header with the filter and send to chosen rx queue (7.1.2.5).
    /// There are up to 128 such filters. If more are needed, will have to enable Flow Director filters.
    /// 
    /// # Argument
    /// * `source_ip`: ipv4 source address
    /// * `dest_ip`: ipv4 destination address
    /// * `source_port`: TCP/UDP/SCTP source port
    /// * `dest_port`: TCP/UDP/SCTP destination port
    /// * `protocol`: IP L4 protocol
    /// * `priority`: priority relative to other filters, can be from 0 (lowest) to 7 (highest)
    /// * `qid`: number of the queue to forward packet to
    pub fn set_5_tuple_filter(&mut self, source_ip: Option<[u8;4]>, dest_ip: Option<[u8;4]>, source_port: Option<u16>, 
        dest_port: Option<u16>, protocol: Option<FilterProtocol>, priority: u8, qid: u8) -> Result<u8, &'static str> 
    {
        if source_ip.is_none() & dest_ip.is_none() & source_port.is_none() & dest_port.is_none() & protocol.is_none() {
            return Err("Must set one of the five filter options");
        }

        if priority > 7 {
            return Err("Protocol cannot be higher than 7");
        }

        let enabled_filters = &mut self.l34_5_tuple_filters;

        // find a free filter
        let filter_num = enabled_filters.iter().position(|&r| r == false).ok_or("Ixgbe: No filter available")?;

        // start off with the filter mask set for all the filters, and clear bits for filters that are enabled
        // bits 29:25 are set to 1.
        let mut filter_mask = 0x3E000000;

        // IP addresses are written to the registers in big endian form (LSB is first on wire)
        // set the source ip address for the filter
        if let Some (addr) = source_ip {
            self.regs3.saqf[filter_num].write(((addr[3] as u32) << 24) | ((addr[2] as u32) << 16) | ((addr[1] as u32) << 8) | (addr[0] as u32));
            filter_mask = filter_mask & !FTQF_SOURCE_ADDRESS_MASK;
        };

        // set the destination ip address for the filter
        if let Some(addr) = dest_ip {
            self.regs3.daqf[filter_num].write(((addr[3] as u32) << 24) | ((addr[2] as u32) << 16) | ((addr[1] as u32) << 8) | (addr[0] as u32));
            filter_mask = filter_mask & !FTQF_DEST_ADDRESS_MASK;
        };        

        // set the source port for the filter    
        if let Some(port) = source_port {
            self.regs3.sdpqf[filter_num].write((port as u32) << SPDQF_SOURCE_SHIFT);
            filter_mask = filter_mask & !FTQF_SOURCE_PORT_MASK;
        };   

        // set the destination port for the filter    
        if let Some(port) = dest_port {
            let port_val = self.regs3.sdpqf[filter_num].read();
            self.regs3.sdpqf[filter_num].write(port_val | (port as u32) << SPDQF_DEST_SHIFT);
            filter_mask = filter_mask & !FTQF_DEST_PORT_MASK;
        };

        // set the filter protocol
        let mut filter_protocol = FilterProtocol::Other;
        if let Some(protocol) = protocol {
            filter_protocol = protocol;
            filter_mask = filter_mask & !FTQF_PROTOCOL_MASK;
        };

        // write the parameters of the filter
        let filter_priority = (priority as u32 & FTQF_PRIORITY) << FTQF_PRIORITY_SHIFT;
        self.regs3.ftqf[filter_num].write(filter_protocol as u32 | filter_priority | filter_mask | FTQF_Q_ENABLE);

        //set the rx queue that the packets for this filter should be sent to
        self.regs3.l34timir[filter_num].write(L34TIMIR_BYPASS_SIZE_CHECK | L34TIMIR_RESERVED | ((qid as u32) << L34TIMIR_RX_Q_SHIFT));

        //mark the filter as used
        enabled_filters[filter_num] = true;
        Ok(filter_num as u8)
    }

    /// Disables the the L3/L4 5-tuple filter for the given filter number
    /// but keeps the values stored in the filter registers.
    fn disable_5_tuple_filter(&mut self, filter_num: u8) {
        // disables filter by setting enable bit to 0
        let val = self.regs3.ftqf[filter_num as usize].read();
        self.regs3.ftqf[filter_num as usize].write(val | !FTQF_Q_ENABLE);

        // sets the record in the nic struct to false
        self.l34_5_tuple_filters[filter_num as usize] = false;
    }

    /// Enable MSI-X interrupts.
    /// Currently all the msi vectors are for packet reception, one msi vector per receive queue.
    /// The assumption here is that we will enable interrupts starting from the first queue in `rxq`
    /// uptil the `NUM_MSI_VEC_ENABLED` queue, and that we are being passed all rx queues starting from queue id 0.
    fn enable_msix_interrupts(regs: &mut IntelIxgbeRegisters1, rxq: &mut Vec<RxQueue<IxgbeRxQueueRegisters,AdvancedRxDescriptor>>, 
        vector_table: &mut MsixVectorTable, interrupt_handlers: &[HandlerFunc]) 
        -> Result<HashMap<u8,u8>, &'static str> 
    {
        if rxq.len() < NUM_MSI_VEC_ENABLED as usize { return Err("Not enough rx queues for the interrupts requested"); }
        // set IVAR reg to enable interrupts for different queues
        // each IVAR register controls 2 RX and 2 TX queues
        let num_queues = NUM_MSI_VEC_ENABLED as usize;
        let queues_per_ivar_reg = 2;
        let enable_interrupt_rx = 0x0080;
        for queue in 0..num_queues {
            let int_enable = 
                // an even number queue which means we'll write to the lower 16 bits
                if queue % queues_per_ivar_reg == 0 {
                    (enable_interrupt_rx | queue) as u32
                }
                // otherwise we'll write to the upper 16 bits
                // need to OR with previous value so that we don't write over a previous queue that's been enabled
                else {
                    ((enable_interrupt_rx | queue) << 16) as u32 | regs.ivar[queue / queues_per_ivar_reg].read()
                };
            regs.ivar[queue / queues_per_ivar_reg].write(int_enable); 
            // debug!("IVAR: {:#X}", regs.ivar.reg[queue / queues_per_ivar_reg].read());
        }
        
        //enable clear on read of EICR and MSI-X mode
        let val = regs.gpie.read();
        regs.gpie.write(val | GPIE_EIMEN | GPIE_MULTIPLE_MSIX | GPIE_PBA_SUPPORT); 
        // debug!("GPIE: {:#X}", regs.gpie.read());

        // set eims bits to enable required interrupt
        // the lower 16 bits are for the 16 receive queue interrupts
        let mut val = 0;
        for i in 0..NUM_MSI_VEC_ENABLED {
            val = val | (EIMS_INTERRUPT_ENABLE << i);
        }
        regs.eims.write(val); 
        // debug!("EIMS: {:#X}", regs.eims.read());

        //enable auto-clear of receive interrupts 
        regs.eiac.write(EIAC_RTXQ_AUTO_CLEAR);

        //clear eicr 
        let _val = regs.eicr.read();
        // debug!("EICR: {:#X}", val);

        // set the throttling time for each interrupt
        // minimum interrupt interval specified in 2us units
        let interrupt_interval = 1; // 2us
        for i in 0..NUM_MSI_VEC_ENABLED as usize {
            regs.eitr[i].write(interrupt_interval << EITR_ITR_INTERVAL_SHIFT);
        }

        let mut interrupt_nums = HashMap::with_capacity(NUM_MSI_VEC_ENABLED as usize);

        // Initialize msi vectors
        for i in 0..NUM_MSI_VEC_ENABLED as usize{ 
            // register an interrupt handler and get an interrupt number that can be used for the msix vector
            let msi_int_num = register_msi_interrupt(interrupt_handlers[i])?;
            interrupt_nums.insert(rxq[i].id, msi_int_num);

            // find core to redirect interrupt to
            // we assume that the number of msi vectors are equal to the number of rx queues
            // TODO: choose a better default value
            let core_id = rxq[i].cpu_id.unwrap_or(0) as u32;
            // unmask the interrupt
            vector_table.msi_vector[i].vector_control.write(MSIX_UNMASK_INT);
            let lower_addr = vector_table.msi_vector[i].msg_lower_addr.read();
            // set the core to which this interrupt will be sent
            vector_table.msi_vector[i].msg_lower_addr.write((lower_addr & !MSIX_ADDRESS_BITS) | MSIX_INTERRUPT_REGION | (core_id << MSIX_DEST_ID_SHIFT)); 
            //allocate an interrupt to msix vector            
            vector_table.msi_vector[i].msg_data.write(msi_int_num as u32);
            // debug!("Created MSI vector: control: {}, core: {}, int: {}", vector_table.msi_vector[i].vector_control.read(), core_id, interrupt_nums[i]);
        }

        Ok(interrupt_nums)
    }

    /// Reads status and clears interrupt
    fn clear_interrupt_status(&self) -> u32 {
        self.regs1.eicr.read()
    }

    /// Removes `num_queues` Rx queues from this "physical" NIC device and gives up ownership of them.
    /// This function is used when creating a virtual NIC that will own the returned queues.
    fn remove_rx_queues(&mut self, num_queues: usize) -> Result<Vec<RxQueue<IxgbeRxQueueRegisters, AdvancedRxDescriptor>>, &'static str> {
        // We always ensure queue 0 is kept for the physical NIC
        if num_queues >= self.rx_queues.len()  {
            return Err("Not enough rx queues for the NIC to remove any");
        }
        let start_remove_index = self.rx_queues.len() - num_queues;
        let queues = self.rx_queues.drain(start_remove_index..).collect(); 
        Ok(queues)
    }

    /// Removes `num_queues` Tx queues from this "physical" NIC device and gives up ownership of them.
    /// This function is when creating a virtual NIC that will own the returned queues.
    fn remove_tx_queues(&mut self, num_queues: usize) -> Result<Vec<TxQueue<IxgbeTxQueueRegisters, AdvancedTxDescriptor>>, &'static str> {
        // We always ensure queue 0 is kept for the physical NIC
        if num_queues >= self.tx_queues.len()  {
            return Err("Not enough tx queues for the NIC to remove any");
        }
        let start_remove_index = self.tx_queues.len() - num_queues;
        let queues = self.tx_queues.drain(start_remove_index..).collect(); 
        Ok(queues)
    }
}

/// Options for the filter protocol used in the 5-tuple filters
pub enum FilterProtocol {
    Tcp = 0,
    Udp = 1,
    Sctp = 2,
    Other = 3
}

/// A helper function to poll the nic receive queues
pub fn rx_poll_mq(qid: usize) -> Result<ReceivedFrame, &'static str> {
    let nic_ref = get_ixgbe_nic().ok_or("ixgbe nic not initialized")?;
    let mut nic = nic_ref.lock();      
    nic.rx_queues[qid as usize].remove_frames_from_queue()?;
    let frame = nic.rx_queues[qid as usize].return_frame().ok_or("no frame")?;
    Ok(frame)
}

/// A helper function to send a test packet on a nic transmit queue
pub fn tx_send_mq(qid: usize) -> Result<(), &'static str> {
    let packet = test_packets::create_test_packet()?;
    let nic_ref = get_ixgbe_nic().ok_or("ixgbe nic not initialized")?;
    let mut nic = nic_ref.lock();  

    nic.tx_queues[qid].send_on_queue(packet);
    Ok(())
}

/// A generic interrupt handler that can be used for packet reception interrupts for any queue.
/// It returns the interrupt number for the rx queue 'qid'.
fn rx_interrupt_handler(qid: u8) -> Option<u8> {
    let interrupt_num = 
        if let Some(ref ixgbe_nic_ref) = IXGBE_NIC.try() {
            let mut ixgbe_nic = ixgbe_nic_ref.lock();
            let _ = ixgbe_nic.rx_queues[qid as usize].remove_frames_from_queue();
            ixgbe_nic.interrupt_num.get(&qid).and_then(|int| Some(*int))
        } else {
            error!("BUG: ixgbe_handler_{}(): IXGBE NIC hasn't yet been initialized!", qid);
            None
        };
    
    interrupt_num
}

/// The interrupt handler for rx queue 0
extern "x86-interrupt" fn ixgbe_handler_0(_stack_frame: &mut ExceptionStackFrame) {
    eoi(rx_interrupt_handler(0));
}
