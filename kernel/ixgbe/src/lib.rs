#![no_std]
#![feature(untagged_unions)]
#![allow(dead_code)] //  to suppress warnings for unused functions/methods
#![allow(safe_packed_borrows)] // temporary, just to suppress unsafe packed borrows 
#![feature(abi_x86_interrupt)]

#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate alloc;
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

pub mod test_ixgbe_driver;
mod regs;
use regs::*;

use spin::Once;
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use irq_safety::MutexIrqSafe;
use alloc::boxed::Box;
use memory::{PhysicalAddress, VirtualAddress, MappedPages};
use pci::{PciDevice, MSIX_CAPABILITY, PciConfigSpaceAccessMechanism};
use bit_field::BitField;
use interrupts::{eoi,register_msi_interrupt};
use x86_64::structures::idt::{ExceptionStackFrame, HandlerFunc};
use hpet::get_hpet;
use network_interface_card::NetworkInterfaceCard;
use nic_initialization::*;
use intel_ethernet::{
    descriptors::{AdvancedRxDescriptor, LegacyTxDescriptor, TxDescriptor, RxDescriptor},
    types::Rdt,
};    
use nic_buffers::{TransmitBuffer, ReceiveBuffer, ReceivedFrame};
use nic_queues::{RxQueue, TxQueue, RxQueueRegisters, TxQueueRegisters};

use owning_ref::BoxRefMut;
use rand::{
    SeedableRng,
    RngCore,
    rngs::SmallRng
};
use runqueue::get_least_busy_core;
use alloc::sync::Arc;
use core::ops::{Deref, DerefMut};
use core::mem::ManuallyDrop;

/// Vendor ID for Intel
pub const INTEL_VEND:                   u16 = 0x8086;  

/// Device ID for the 82599ES, used to identify the device from the PCI space
pub const INTEL_82599:                  u16 = 0x10FB;  


/** Default configuration at time of initialization of NIC **/
/// If interrupts are enabled for packet reception
const INTERRUPT_ENABLE:                     bool    = false;
/// The number of MSI vectors enabled for the NIC, with the maximum for the 82599 being 64.
/// This number is only relevant if interrupts are enabled.
const NUM_MSI_VEC_ENABLED:                  u8      = 1; //IXGBE_NUM_RX_QUEUES_ENABLED;
/// We do not access the PHY module for link information yet,
/// so this variable is set by the user depending of if the attached module is 10GB or 1GB.
const IXGBE_10GB_LINK:                      bool    = false;
/// If link uses 1GB SFP modules
const IXGBE_1GB_LINK:                       bool    = !(IXGBE_10GB_LINK);
/// The number of receive descriptors per queue
const IXGBE_NUM_RX_DESC:                    u16     = 8;
/// The number of transmit descriptors per queue
const IXGBE_NUM_TX_DESC:                    u16     = 8;
/// If receive side scaling (where incoming packets are sent to different queues depending on a hash) is enabled.
const RSS_ENABLE:                           bool    = false;
/// The number of receive queues that are enabled.
/// It can be a a maximum of 16 since we are using only the physical functions.
/// If RSS or filters are not enabled, this should be 1.
const IXGBE_NUM_RX_QUEUES_ENABLED:          u8      = 8;
/// The maximum number of rx queues available on this NIC (virtualization has to be enabled)
const IXGBE_MAX_RX_QUEUES:                  u8      = 128;
/// The number of transmit queues that are enabled. 
/// Support for multiple Tx queues hasn't been added so this should remain 1.
const IXGBE_NUM_TX_QUEUES_ENABLED:          u8      = 1;
/// The maximum number of tx queues available on this NIC (virtualization has to be enabled)
const IXGBE_MAX_TX_QUEUES:                  u8      = 128;
/// Size of the Rx packet buffers
const IXGBE_RX_BUFFER_SIZE_IN_BYTES:        u16     = 8192;


/// The single instance of the 82599 NIC.
pub static IXGBE_NIC: Once<MutexIrqSafe<IxgbeNic>> = Once::new();

/// Returns a reference to the IxgbeNic wrapped in a RwLockIrqSafe,
/// if it exists and has been initialized.
pub fn get_ixgbe_nic() -> Option<&'static MutexIrqSafe<IxgbeNic>> {
    IXGBE_NIC.try()
}

/// How many ReceiveBuffers are preallocated for this driver to use. 
const RX_BUFFER_POOL_SIZE:                  usize = IXGBE_NUM_RX_QUEUES_ENABLED as usize * IXGBE_NUM_RX_DESC as usize * 2; 

lazy_static! {
    /// The pool of pre-allocated receive buffers that are used by the IXGBE NIC
    /// and temporarily given to higher layers in the networking stack.
    static ref RX_BUFFER_POOL: mpmc::Queue<ReceiveBuffer> = mpmc::Queue::with_capacity(RX_BUFFER_POOL_SIZE * 2);
}

struct IxgbeRxQueueRegisters {
    /// We prevent the drop handler from dropping the `regs` because the backing memory is not on the heap, but in the stored mapped pages.
    /// The memory will be deallocated when the `backing_pages` are dropped.
    regs: ManuallyDrop<Box<RegistersRx>>,
    backing_pages: Arc<MappedPages>
}
impl RxQueueRegisters for IxgbeRxQueueRegisters {
    fn update_rdbal(&mut self, value: u32) {
        self.regs.rdbal.write(value)
    }    
    fn update_rdbah(&mut self, value: u32) {
        self.regs.rdbah.write(value)
    }
    fn update_rdlen(&mut self, value: u32) {
        self.regs.rdlen.write(value)
    }
    fn update_rdh(&mut self, value: u32) {
        self.regs.rdh.write(value)
    }
    fn update_rdt(&mut self, value: u32) {
        self.regs.rdt.write(value)
    }
}
impl Deref for IxgbeRxQueueRegisters {
    type Target = Box<RegistersRx>;
    fn deref(&self) -> &Box<RegistersRx> {
        &self.regs
    }
}
impl DerefMut for IxgbeRxQueueRegisters {
    fn deref_mut(&mut self) -> &mut Box<RegistersRx> {
        &mut self.regs
    }
}


struct IxgbeTxQueueRegisters {
    /// We prevent the drop handler from dropping the `regs` because the backing memory is not on the heap, but in the stored mapped pages.
    /// The memory will be deallocated when the `backing_pages` are dropped.
    regs: ManuallyDrop<Box<RegistersTx>>,
    backing_pages: Arc<MappedPages>
}
impl TxQueueRegisters for IxgbeTxQueueRegisters {
    fn update_tdbal(&mut self, value: u32) {
        self.regs.tdbal.write(value)
    }    
    fn update_tdbah(&mut self, value: u32) {
        self.regs.tdbah.write(value)
    }
    fn update_tdlen(&mut self, value: u32) {
        self.regs.tdlen.write(value)
    }
    fn update_tdh(&mut self, value: u32) {
        self.regs.tdh.write(value)
    }
    fn update_tdt(&mut self, value: u32) {
        self.regs.tdt.write(value)
    }
}
impl Deref for IxgbeTxQueueRegisters {
    type Target = Box<RegistersTx>;
    fn deref(&self) -> &Box<RegistersTx> {
        &self.regs
    }
}
impl DerefMut for IxgbeTxQueueRegisters {
    fn deref_mut(&mut self) -> &mut Box<RegistersTx> {
        &mut self.regs
    }
}


/// A struct representing an ixgbe network interface card
pub struct IxgbeNic {
    /// Type of Base Address Register 0,
    /// if it's memory mapped or I/O.
    bar_type: u8,
    /// MMIO Base Address     
    mem_base: PhysicalAddress,
    /// Interrupt number for each msi vector
    interrupt_num: Option<[u8; NUM_MSI_VEC_ENABLED as usize]>,
    /// The actual MAC address burnt into the hardware  
    mac_hardware: [u8;6],       
    /// The optional spoofed MAC address to use in place of `mac_hardware` when transmitting.  
    mac_spoofed: Option<[u8; 6]>,
    /// Memory-mapped control registers
    regs1: BoxRefMut<MappedPages, IntelIxgbeRegisters1>,
    regs2: BoxRefMut<MappedPages, IntelIxgbeRegisters2>,
    regs3: BoxRefMut<MappedPages, IntelIxgbeRegisters3>,
    regs_mac: BoxRefMut<MappedPages, IntelIxgbeMacRegisters>,
    /// Memory-mapped msi-x vector table
    msix_vector_table: BoxRefMut<MappedPages, MsixVectorTable>,
    /// Array to store which L3/L4 5-tuple filters have been used.
    /// There are 128 such filters available.
    l34_5_tuple_filters: [bool; 128],
    /// The number of rx queues enabled
    num_rx_queues: u8,
    /// Vector of all the rx queues
    rx_queues: Vec<RxQueue<IxgbeRxQueueRegisters,AdvancedRxDescriptor>>,
    /// Registers for the unused queues
    rx_registers_unused: Vec<IxgbeRxQueueRegisters>,
    /// The number of tx queues enabled
    num_tx_queues: u8,
    /// Vector of all the tx queues
    tx_queues: Vec<TxQueue<IxgbeTxQueueRegisters,LegacyTxDescriptor>>,
    /// Registers for the unused queues
    tx_registers_unused: Vec<IxgbeTxQueueRegisters>,
}

// A trait which contains common functionalities for a NIC
impl NetworkInterfaceCard for IxgbeNic {

    fn send_packet(&mut self, transmit_buffer: TransmitBuffer) -> Result<(), &'static str> {
        // by default, when using the physical NIC interface, we send on queue 0.
        let qid = 0;
        self.tx_queues[qid].send_on_queue(transmit_buffer);
        Ok(())
    }

    // this function has only been tested with 1 Rx queue and is meant to be used with the smoltcp stack.
    fn get_received_frame(&mut self) -> Option<ReceivedFrame> {
        // by default, when using the physical NIC interface, we receive on queue 0.
        let qid = 0;
        // return one frame from the queue's received frames
        self.rx_queues[0].received_frames.pop_front()
    }

    fn poll_receive(&mut self) -> Result<(), &'static str> {
        // by default, when using the physical NIC interface, we receive on queue 0.
        let qid = 0;
        self.rx_queues[qid].remove_frames_from_queue()?;
        Ok(())
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
        // map the IntelIxgbeRegisters struct to the address found from the pci space
        let (mut mapped_registers1, mut mapped_registers2, mut mapped_registers3, mut mapped_registers_mac, 
            mut rx_mapped_registers, mut tx_mapped_registers) = Self::mapped_reg(ixgbe_pci_dev, mem_base)?;

        // map the msi-x vector table to an address found from the pci space
        let mut vector_table = Self::mem_map_msix(ixgbe_pci_dev)?;

        // link initialization
        Self::start_link(&mut mapped_registers1, &mut mapped_registers2, &mut mapped_registers3, &mut mapped_registers_mac)?;

        // store the mac address of this device
        let mac_addr_hardware = Self::read_mac_address_from_nic(&mut mapped_registers_mac);

        // initialize the buffer pool
        if let Err(e) = init_rx_buf_pool(RX_BUFFER_POOL_SIZE, IXGBE_RX_BUFFER_SIZE_IN_BYTES, &RX_BUFFER_POOL){
            error!("{}", e);
        }

        // create the rx desc queues and their packet buffers
        let (mut rx_descs, mut rx_buffers) = Self::rx_init(&mut mapped_registers2, &mut rx_mapped_registers)?;
        // create the vec of rx queues
        let mut rx_queues = Vec::new();
        let mut id = 0;
        while !rx_descs.is_empty() {
            // let cpu_id = get_least_busy_core().ok_or("ixgbe::init: No core available")?;
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
        let mut tx_descs = Self::tx_init(&mut mapped_registers2, &mut tx_mapped_registers)?;
        // create the vec of tx queues
        let mut tx_queues = Vec::new();
        let mut id = 0;
        while !tx_descs.is_empty() {
            // let cpu_id = get_least_busy_core().ok_or("ixgbe::init: No core available")?;
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
                Some(Self::enable_msix_interrupts(&mut mapped_registers1, &mut rx_queues, &mut vector_table, &interrupt_handlers)?)
            }
            else {
                None
            };

        // enable Receive Side Scaling if required
        if RSS_ENABLE {
            Self::setup_mrq(&mut mapped_registers2, &mut mapped_registers3)?;
        }

        let ixgbe_nic = IxgbeNic {
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
            l34_5_tuple_filters: [false; 128],
            num_rx_queues: IXGBE_NUM_RX_QUEUES_ENABLED,
            rx_queues: rx_queues,
            rx_registers_unused: rx_mapped_registers,
            num_tx_queues: IXGBE_NUM_TX_QUEUES_ENABLED,
            tx_queues: tx_queues,
            tx_registers_unused: tx_mapped_registers,
        };

        let nic_ref = IXGBE_NIC.call_once(|| MutexIrqSafe::new(ixgbe_nic));
        Ok(nic_ref)
    }

    /// Returns the memory mapped registers of the nic
    fn mapped_reg (dev: &PciDevice, mem_base: PhysicalAddress) 
        -> Result<(BoxRefMut<MappedPages, IntelIxgbeRegisters1>, BoxRefMut<MappedPages, IntelIxgbeRegisters2>, BoxRefMut<MappedPages, IntelIxgbeRegisters3>, 
            BoxRefMut<MappedPages, IntelIxgbeMacRegisters>, Vec<IxgbeRxQueueRegisters>, Vec<IxgbeTxQueueRegisters>), &'static str> {
        
        let GENERAL_REGISTERS_1_SIZE_BYTES = 4096;
        let RX_REGISTERS_SIZE_BYTES = 4096;
        let GENERAL_REGISTERS_2_SIZE_BYTES = 4 * 4096;
        let TX_REGISTERS_SIZE_BYTES = 8192;
        let MAC_REGISTERS_SIZE_BYTES = 5 * 4096;
        let GENERAL_REGISTERS_3_SIZE_BYTES = 18 * 4096;

        let nic_regs1_mapped_page = allocate_memory(mem_base, GENERAL_REGISTERS_1_SIZE_BYTES)?;
        let nic_rx_regs1_mapped_page = allocate_memory(mem_base + GENERAL_REGISTERS_1_SIZE_BYTES, RX_REGISTERS_SIZE_BYTES)?;
        let nic_regs2_mapped_page = allocate_memory(mem_base + GENERAL_REGISTERS_1_SIZE_BYTES + RX_REGISTERS_SIZE_BYTES, GENERAL_REGISTERS_2_SIZE_BYTES)?;        
        let nic_tx_regs_mapped_page = allocate_memory(mem_base + GENERAL_REGISTERS_1_SIZE_BYTES + RX_REGISTERS_SIZE_BYTES + GENERAL_REGISTERS_2_SIZE_BYTES, TX_REGISTERS_SIZE_BYTES)?;
        let nic_mac_regs_mapped_page = allocate_memory(mem_base + GENERAL_REGISTERS_1_SIZE_BYTES + RX_REGISTERS_SIZE_BYTES + GENERAL_REGISTERS_2_SIZE_BYTES + TX_REGISTERS_SIZE_BYTES, MAC_REGISTERS_SIZE_BYTES)?;
        let nic_rx_regs2_mapped_page = allocate_memory(mem_base + GENERAL_REGISTERS_1_SIZE_BYTES + RX_REGISTERS_SIZE_BYTES + GENERAL_REGISTERS_2_SIZE_BYTES + TX_REGISTERS_SIZE_BYTES + MAC_REGISTERS_SIZE_BYTES, RX_REGISTERS_SIZE_BYTES)?;        
        let nic_regs3_mapped_page = allocate_memory(mem_base + GENERAL_REGISTERS_1_SIZE_BYTES + RX_REGISTERS_SIZE_BYTES + GENERAL_REGISTERS_2_SIZE_BYTES + TX_REGISTERS_SIZE_BYTES + MAC_REGISTERS_SIZE_BYTES + RX_REGISTERS_SIZE_BYTES, GENERAL_REGISTERS_3_SIZE_BYTES)?;

        let regs1 = BoxRefMut::new(Box::new(nic_regs1_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<IntelIxgbeRegisters1>(0))?;
        let regs2 = BoxRefMut::new(Box::new(nic_regs2_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<IntelIxgbeRegisters2>(0))?;
        let regs3 = BoxRefMut::new(Box::new(nic_regs3_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<IntelIxgbeRegisters3>(0))?;
        let mac_regs = BoxRefMut::new(Box::new(nic_mac_regs_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<IntelIxgbeMacRegisters>(0))?;
        let mut regs_rx = Self::mapped_regs_from_rx_memory(nic_rx_regs1_mapped_page);
        regs_rx.append(&mut Self::mapped_regs_from_rx_memory(nic_rx_regs2_mapped_page));
        let regs_tx = Self::mapped_regs_from_tx_memory(nic_tx_regs_mapped_page);
            
        Ok((regs1, regs2, regs3, mac_regs, regs_rx, regs_tx))
    }

    fn mapped_regs_from_rx_memory(mp: MappedPages) -> Vec<IxgbeRxQueueRegisters> {
        const QUEUES_IN_MP: usize = 64;
        const RX_QUEUE_REGISTERS_SIZE_BYTES: usize = core::mem::size_of::<RegistersRx>();
        
        assert!(mp.size_in_bytes() >= QUEUES_IN_MP * RX_QUEUE_REGISTERS_SIZE_BYTES);

        let starting_address = mp.start_address();
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

    fn mapped_regs_from_tx_memory(mp: MappedPages) -> Vec<IxgbeTxQueueRegisters> {
        const QUEUES_IN_MP: usize = 128;
        const TX_QUEUE_REGISTERS_SIZE_BYTES: usize = core::mem::size_of::<RegistersTx>();
        
        assert!(mp.size_in_bytes() >= QUEUES_IN_MP * TX_QUEUE_REGISTERS_SIZE_BYTES);

        let starting_address = mp.start_address();
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
    fn release_semaphore(regs: &mut IntelIxgbeRegisters3) -> Result<(), &'static str> {
        // clear bit of released resource
        let sw_fw_sync = regs.sw_fw_sync.read() & !(SW_FW_SYNC_SW_MAC);
        regs.sw_fw_sync.write(sw_fw_sync);

        // release semaphore
        let _swsm = regs.swsm.read() & !(SWSM_SMBI) & !(SWSM_SWESMBI);

        Ok(())
    }

    /// Software reset of NIC to get it running
    fn start_link (mut regs1: &mut IntelIxgbeRegisters1, mut regs2: &mut IntelIxgbeRegisters2, mut regs3: &mut IntelIxgbeRegisters3, mut regs_mac: &mut IntelIxgbeMacRegisters) -> Result<(), &'static str>{
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
        for fcttv in regs2.fcttv.reg.iter_mut() {
            fcttv.write(0);
        }

        for fcrtl in regs2.fcrtl.reg.iter_mut() {
            fcrtl.write(0);
        }

        for fcrth in regs2.fcrth.reg.iter_mut() {
            fcrth.write(0);
        }

        regs2.fcrtv.write(0);
        regs2.fccfg.write(0);

        //disable interrupts
        regs1.eims.write(DISABLE_INTERRUPTS);

        //wait for eeprom auto read completion?

        //read MAC address
        debug!("Ixgbe: MAC address low: {:#X}", regs_mac.ral.read());
        debug!("Ixgbe: MAC address high: {:#X}", regs_mac.rah.read() & 0xFFFF);

        //wait for dma initialization done (RDRXCTL.DMAIDONE)
        let mut val = regs2.rdrxctl.read();
        let dmaidone_bit = 1 << 3;
        while val & dmaidone_bit != dmaidone_bit {
            val = regs2.rdrxctl.read();
        }

        while Self::acquire_semaphore(&mut regs3)? {
            //wait 10 ms
            let _ =pit_clock::pit_wait(wait_time);
        }

        //setup PHY and the link 
        if IXGBE_10GB_LINK {
            let val = regs2.autoc.read();
            regs2.autoc.write(val | AUTOC_LMS_10_GBE_S); // value should be 0xC09C_6004

            let val = regs2.autoc2.read();
            regs2.autoc2.write(val | AUTOC2_10G_PMA_PMD_S_SFI); // value should be 0xA_0000
        }
        else {
            let mut val = regs2.autoc.read();
            val = (val & !(AUTOC_LMS_1_GB) & !(AUTOC_1G_PMA_PMD)) | AUTOC_FLU;
            regs2.autoc.write(val);
        }

        let val = regs2.autoc.read();
        regs2.autoc.write(val|AUTOC_RESTART_AN); 

        Self::release_semaphore(&mut regs3)?;        

        // debug!("STATUS: {:#X}", regs.status.read()); 
        // debug!("CTRL: {:#X}", regs.ctrl.read());
        // debug!("LINKS: {:#X}", regs.links.read()); //b7 and b30 should be 1 for link up 
        // debug!("AUTOC: {:#X}", regs.autoc.read()); 
        // debug!("AUTOC2: {:#X}", regs.autoc2.read()); 

        Ok(())
    }

    /// Initializes the array of receive descriptors and their corresponding receive buffers,
    /// and returns a tuple including both of them for all rx queues in use.
    fn rx_init(regs: &mut IntelIxgbeRegisters2,rx_regs: &mut Vec<IxgbeRxQueueRegisters>) -> Result<(Vec<BoxRefMut<MappedPages, [AdvancedRxDescriptor]>>, Vec<Vec<ReceiveBuffer>>), &'static str>  {

        let mut rx_descs_all_queues = Vec::new();
        let mut rx_bufs_in_use_all_queues = Vec::new();
        
        for qid in 0..IXGBE_NUM_RX_QUEUES_ENABLED {
            let rxq = &mut rx_regs[qid as usize];
            // get the queue of rx descriptors and their corresponding rx buffers
            let (rx_descs, rx_bufs_in_use) = init_rx_queue(IXGBE_NUM_RX_DESC as usize, &RX_BUFFER_POOL, IXGBE_RX_BUFFER_SIZE_IN_BYTES as usize, rxq)?;          
            
            //set the size of the packet buffers and the descriptor format used
            let mut val = rxq.srrctl.read();
            val.set_bits(0..4, BSIZEPACKET_8K);
            val.set_bits(8..13, BSIZEHEADER_256B);
            val.set_bits(25..27, DESCTYPE_ADV_1BUFFER);
            rxq.srrctl.write(val);

            //enable the rx queue
            let mut val = rxq.rxdctl.read();
            val.set_bit(25, RX_Q_ENABLE);
            rxq.rxdctl.write(val);

            //make sure queue is enabled
            while rxq.rxdctl.read().get_bit(25) != RX_Q_ENABLE {}
            
            // Write the tail index.
            // Note that the 82599 datasheet (section 8.2.3.8.5) states that we should set the RDT (tail index) to the index *beyond* the last receive descriptor, 
            // but we set it to the last receive descriptor for the same reason as the e1000 driver
            rxq.rdt.write((IXGBE_NUM_RX_DESC - 1) as u32);
            
            rx_descs_all_queues.push(rx_descs);
            rx_bufs_in_use_all_queues.push(rx_bufs_in_use);
        }
        
        // set rx parameters of which type of packets are accepted by the nic
        // right now we allow the nic to receive all types of packets, even incorrectly formed ones
        regs.fctrl.write(STORE_BAD_PACKETS | MULTICAST_PROMISCUOUS_ENABLE | UNICAST_PROMISCUOUS_ENABLE | BROADCAST_ACCEPT_MODE); 
        
        // enable receive functionality
        let val = regs.rxctrl.read();
        regs.rxctrl.write(val | RECEIVE_ENABLE); 

        Ok((rx_descs_all_queues, rx_bufs_in_use_all_queues))
    }

    /// Enable multiple receive queues with RSS.
    /// Part of queue initialization is done in the rx_init function.
    pub fn setup_mrq(regs2: &mut IntelIxgbeRegisters2, regs3: &mut IntelIxgbeRegisters3) -> Result<(), &'static str>{
        // enable RSS writeback in the header field of the receive descriptor
        regs2.rxcsum.write(RXCSUM_PCSD);
        
        // enable RSS and set fields that will be used by hash function
        // right now we're using the udp port and ipv4 address.
        regs3.mrqc.write(MRQC_MRQE_RSS | MRQC_UDPIPV4 ); 

        //set the random keys for the hash function
        let seed = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
        let mut rng = SmallRng::seed_from_u64(seed);
        for rssrk in regs3.rssrk.reg.iter_mut() {
            rssrk.write(rng.next_u32());
        }

        // Initialize the RSS redirection table
        // each reta register has 4 redirection entries
        // since mapping to queues is random and based on a hash, we randomly assign 1 queue to each reta register
        let mut qid = 0;
        for reta in regs3.reta.reg.iter_mut() {
            //set 4 entries to the same queue number
            let val = qid << RETA_ENTRY_0_OFFSET | qid << RETA_ENTRY_1_OFFSET | qid << RETA_ENTRY_2_OFFSET | qid << RETA_ENTRY_3_OFFSET;
            reta.write(val);

            // next 4 entries will be assigned to the next queue
            qid = (qid + 1) % IXGBE_NUM_RX_QUEUES_ENABLED as u32;
        }

        Ok(())
    }


    /// Enables Direct Cache Access for the device.
    /// TODO: need to see if to allow DCA from this device, the identification number has to be programmed into the chipset register
    fn enable_dca(regs: &mut IntelIxgbeRegisters3, rxq: &mut Vec<RxQueue<IxgbeRxQueueRegisters,AdvancedRxDescriptor>>) -> Result<(), &'static str> {
        // Enable DCA tagging, which writes the cpu id to the PCIe Transaction Layer Packets (TLP)
        // There are 2 version of DCA that are mentioned, legacy and 1.0
        // We always enable 1.0 since (1) currently haven't found additional info online and (2) the linux driver always enables 1.0 
        regs.dca_ctrl.write(DCA_MODE_2 | DCA_ENABLE);
        Self::enable_rx_dca(rxq)  
    }


    /// Sets up DCA for the rx queues that have been enabled
    /// you can optionally choose to have the descriptor, header and payload copied to the cache for each received packet
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
            rxq.regs.dca_rxctrl.write(RX_DESC_DCA_ENABLE | RX_DESC_R_RELAX_ORDER_EN | (cpu_id << DCA_CPUID_SHIFT));
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
    /// * `protocol`: tcp = 0, udp = 1, sctp = 2, other = 3
    /// * `priority`: priority relative to other filters, can be from 0 (lowest) to 7 (highest)
    /// * `qid`: number of the queue to forward packet to
    pub fn set_5_tuple_filter(&mut self, source_ip: [u8;4], dest_ip: [u8;4], source_port: u16, dest_port: u16, protocol: u8, priority: u8, qid: u8) -> Result<u8, &'static str> {
        let enabled_filters = &mut self.l34_5_tuple_filters;

        // find a free filter
        let filter_num = enabled_filters.iter().position(|&r| r == false).ok_or("Ixgbe: No filter available")?;

        // IP addresses are written to the registers in big endian form (LSB is first on wire)
        // set the source ip address for the filter
        self.regs3.saqf.reg[filter_num].write(((source_ip[3] as u32) << 24) | ((source_ip[2] as u32) << 16) | ((source_ip[1] as u32) << 8) | (source_ip[0] as u32));
        // set the destination ip address for the filter        
        self.regs3.daqf.reg[filter_num].write(((dest_ip[3] as u32) << 24) | ((dest_ip[2] as u32) << 16) | ((dest_ip[1] as u32) << 8) | (dest_ip[0] as u32));
        // set the source and destination ports for the filter        
        self.regs3.sdpqf.reg[filter_num].write(((source_port as u32) << SPDQF_SOURCE_SHIFT) | ((dest_port as u32) << SPDQF_DEST_SHIFT));

        // set up the parameters of the filter
        let filter_protocol = protocol as u32 & FTQF_PROTOCOL;
        let filter_priority = (priority as u32 & FTQF_PRIORITY) << FTQF_PRIORITY_SHIFT;
        let filter_mask = FTQF_DEST_PORT_MASK | FTQF_SOURCE_PORT_MASK | FTQF_SOURCE_ADDRESS_MASK;
        self.regs3.ftqf.reg[filter_num].write(filter_protocol | filter_priority | filter_mask | FTQF_Q_ENABLE);

        //set the rx queue that the packets for this filter should be sent to
        self.regs3.l34timir.reg[filter_num].write(L34TIMIR_BYPASS_SIZE_CHECK | L34TIMIR_RESERVED | ((qid as u32) << L34TIMIR_RX_Q_SHIFT));

        //mark the filter as used
        enabled_filters[filter_num] = true;
        error!("set filter {}", filter_num);
        Ok(filter_num as u8)
    }

    /// Disables the the L3/L4 5-tuple filter for the given filter number
    /// but keeps the values stored in the filter registers.
    fn disable_5_tuple_filter(&mut self, filter_num: u8) {
        // disables filter by setting enable bit to 0
        let val = self.regs3.ftqf.reg[filter_num as usize].read();
        self.regs3.ftqf.reg[filter_num as usize].write(val | !FTQF_Q_ENABLE);

        // sets the record in the nic struct to false
        self.l34_5_tuple_filters[filter_num as usize] = false;
    }

    /// Initialize the array of tramsmit descriptors and return them.
    fn tx_init(regs: &mut IntelIxgbeRegisters2, tx_regs: &mut Vec<IxgbeTxQueueRegisters>) -> Result<Vec<BoxRefMut<MappedPages, [LegacyTxDescriptor]>>, &'static str>   {
        //disable transmission
        let val = regs.dmatxctl.read();
        regs.dmatxctl.write(val & !TE); 

        // only initialize 1 queue for now
        let qid = 0;
        let txq = &mut tx_regs[qid as usize];

        let tx_descs = init_tx_queue(IXGBE_NUM_TX_DESC as usize, txq)?;
        
        // enable transmit operation
        let val = regs.dmatxctl.read();
        regs.dmatxctl.write(val | TE); 
        
        //enable tx queue
        let mut val = txq.txdctl.read();
        val.set_bit(25, TX_Q_ENABLE);
        txq.txdctl.write(val); 

        //make sure queue is enabled
        while txq.txdctl.read().get_bit(25) != TX_Q_ENABLE {} 

        Ok(vec![tx_descs])
    }  

    /// Enable MSI-X interrupts.
    /// Currently all the msi vectors are for packet reception, one msi vector per receive queue.
    fn enable_msix_interrupts(regs: &mut IntelIxgbeRegisters1, rxq: &mut Vec<RxQueue<IxgbeRxQueueRegisters,AdvancedRxDescriptor>>, vector_table: &mut MsixVectorTable, interrupt_handlers: &[HandlerFunc]) -> Result<[u8; NUM_MSI_VEC_ENABLED as usize], &'static str> {
        // set IVAR reg to enable interrupts for different queues
        // each IVAR register controls 2 RX and 2 TX queues
        let num_queues = IXGBE_NUM_RX_QUEUES_ENABLED as usize;
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
                    ((enable_interrupt_rx | queue) << 16) as u32 | regs.ivar.reg[queue / queues_per_ivar_reg].read()
                };
            regs.ivar.reg[queue / queues_per_ivar_reg].write(int_enable); 
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
            regs.eitr.reg[i].write(interrupt_interval << EITR_ITR_INTERVAL_SHIFT);
        }

        let mut interrupt_nums = [0; NUM_MSI_VEC_ENABLED as usize];

        // Initialize msi vectors
        for i in 0..NUM_MSI_VEC_ENABLED as usize{ 
            // register an interrupt handler and get an interrupt number that can be used for the msix vector
            interrupt_nums[i] = register_msi_interrupt(interrupt_handlers[i])?;

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
            vector_table.msi_vector[i].msg_data.write(interrupt_nums[i] as u32);
            // debug!("Created MSI vector: control: {}, core: {}, int: {}", vector_table.msi_vector[i].vector_control.read(), core_id, interrupt_nums[i]);
        }

        Ok(interrupt_nums)
    }

    /// Reads status and clears interrupt
    fn clear_interrupt_status(&self) -> u32 {
        self.regs1.eicr.read()
    }

    // /// Returns all the receive buffers in one packet,
    // /// called for individual queues
    // pub fn handle_receive<T: RxDescriptor>(&self, mut rxq: &mut RxQueue<T>) -> Result<(), &'static str> {
    //     Self::remove_frames_from_queue::<T>(&mut rxq, IXGBE_NUM_RX_DESC as u16, &RX_BUFFER_POOL, IXGBE_RX_BUFFER_SIZE_IN_BYTES)
    // }   
    
    // /// Transmits a packet on the given tx queue
    // fn handle_transmit<T:TxDescriptor>(&self, mut txq: &mut TxQueue<T>, transmit_buffer: TransmitBuffer) -> Result<(), &'static str> {
    //     Self::send_on_queue::<T>(&mut txq, IXGBE_NUM_TX_DESC as u16, transmit_buffer);
    //     Ok(())
    // }

    /// Collects all the packets for a given queue on a rx interrupt
    fn handle_rx_interrupt(&mut self, qid: u8) {
        // TODO: we should check length
        if let Err(e) = self.rx_queues[qid as usize].remove_frames_from_queue() {
            error!("handle_rx_interrupt(): error handling interrupt: {:?}", e);
        }    
    }

}



/// A helper function to poll the nic receive queues
pub fn rx_poll_mq(qid: usize) -> Result<(), &'static str> {
    let nic_ref = get_ixgbe_nic().ok_or("ixgbe nic not initialized")?;
    let mut nic = nic_ref.lock();      
    nic.rx_queues[qid as usize].remove_frames_from_queue()
}

/// A generic interrupt handler that can be used for packet reception interrupts for any queue.
/// It returns the interrupt number for the rx queue 'qid'.
fn rx_interrupt_handler(qid: u8) -> Option<u8> {
    let interrupt_num = 
        if let Some(ref ixgbe_nic_ref) = IXGBE_NIC.try() {
            let mut ixgbe_nic = ixgbe_nic_ref.lock();
            ixgbe_nic.handle_rx_interrupt(qid);
            // this handler will only be registered if interrupts are enabled
            // so we can use unwrap() in this case
            Some(ixgbe_nic.interrupt_num.unwrap()[qid as usize])
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










