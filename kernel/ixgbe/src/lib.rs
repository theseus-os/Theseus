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
extern crate network_interface_card;
extern crate owning_ref;
extern crate rand;
extern crate hpet;
extern crate runqueue;

pub mod test_ixgbe_driver;
pub mod registers;

use core::ptr::{read_volatile, write_volatile};
use core::ops::DerefMut;
use spin::Once;
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use irq_safety::MutexIrqSafe;
use alloc::boxed::Box;
use memory::{get_kernel_mmi_ref,FRAME_ALLOCATOR, PhysicalAddress, VirtualAddress, FrameRange, EntryFlags, MappedPages, PhysicalMemoryArea, allocate_pages_by_bytes, create_contiguous_mapping};
use pci::{PciDevice,pci_read_32, pci_write, pci_set_command_bus_master_bit,pci_set_interrupt_disable_bit, PCI_BAR0, pci_enable_msix, pci_config_space, MSIX_CAPABILITY};
use kernel_config::memory::PAGE_SIZE;
use registers::*;
use bit_field::BitField;
use interrupts::{eoi,register_msi_interrupt};
use x86_64::structures::idt::{ExceptionStackFrame, HandlerFunc};
use apic::get_my_apic_id;
use hpet::get_hpet;
use network_interface_card::{
    {NetworkInterfaceCard, TransmitBuffer, ReceiveBuffer, ReceivedFrame, nic_mapping_flags},
    intel_ethernet::{NicInit, AdvancedRxDesc, LegacyTxDesc, TxDescriptor},
};
use owning_ref::BoxRefMut;
use rand::{
    SeedableRng,
    RngCore,
    rngs::SmallRng
};
use runqueue::get_least_busy_core;


/* Default configuration at time of initialization of NIC */
const INTERRUPT_ENABLE:                     bool    = false;
const NUM_MSI_VEC_ENABLED:                  usize   = 1;
const IXGBE_10GB_LINK:                      bool    = true;
const IXGBE_1GB_LINK:                       bool    = !(IXGBE_10GB_LINK);
const IXGBE_NUM_RX_DESC:                    usize   = 1024;
const IXGBE_NUM_TX_DESC:                    usize   = 8;
const RSS_ENABLE:                           bool    = false;
const IXGBE_NUM_RX_QUEUES:                  u8      = 1;
const IXGBE_NUM_TX_QUEUES:                  u8      = 1;
const IXGBE_RX_BUFFER_SIZE_IN_BYTES:        u16     = 8192;
const IXGBE_RX_HEADER_SIZE_IN_BYTES:        u16     = 256;
const IXGBE_TX_BUFFER_SIZE_IN_BYTES:        usize   = 256;


/// The single instance of the 82599 NIC.
pub static IXGBE_NIC: Once<MutexIrqSafe<IxgbeNic>> = Once::new();
/// The single instance of Rx Descriptor Queues for the NIC
pub static IXGBE_RX_QUEUES: Once<RxQueues> = Once::new();
/// The single instance of Tx Descriptor Queues for the NIC
pub static IXGBE_TX_QUEUES: Once<TxQueues> = Once::new();

/// Returns a reference to the IxgbeNic wrapped in a MutexIrqSafe,
/// if it exists and has been initialized.
pub fn get_ixgbe_nic() -> Option<&'static MutexIrqSafe<IxgbeNic>> {
    IXGBE_NIC.try()
}

/// Returns a reference to the Rx descriptor queues
pub fn get_ixgbe_rx_queues() -> Option<&'static RxQueues> {
    IXGBE_RX_QUEUES.try()
}

/// Returns a reference to the Tx descriptor queues
pub fn get_ixgbe_tx_queues() -> Option<&'static TxQueues> {
    IXGBE_TX_QUEUES.try()
}

/// How many ReceiveBuffers are preallocated for this driver to use. 
const RX_BUFFER_POOL_SIZE:                  usize = 256; 

lazy_static! {
    /// The pool of pre-allocated receive buffers that are used by the IXGBE NIC
    /// and temporarily given to higher layers in the networking stack.
    static ref RX_BUFFER_POOL: mpmc::Queue<ReceiveBuffer> = mpmc::Queue::with_capacity(RX_BUFFER_POOL_SIZE);
}

/// A struct representing an ixgbe network interface card
pub struct IxgbeNic {
    /// Type of BAR0
    bar_type: u8,
    /// MMIO Base Address     
    mem_base: PhysicalAddress,
    /// MMIO Base virtual address
    mem_base_v: VirtualAddress,   
    /// A flag indicating if eeprom exists
    eeprom_exists: bool,
    /// Interrupt number for each msi vector
    interrupt_num: Option<[u8; NUM_MSI_VEC_ENABLED]>,
    /// The actual MAC address burnt into the hardware  
    mac_hardware: [u8;6],       
    /// The optional spoofed MAC address to use in place of `mac_hardware` when transmitting.  
    mac_spoofed: Option<[u8; 6]>,
    /// Memory-mapped control registers
    regs: BoxRefMut<MappedPages, IntelIxgbeRegisters>,
    /// Memory-mapped msi-x vector table
    msix_vector_table: BoxRefMut<MappedPages, MsixVectorTable>,
    /// Array to store which L3/L4 5-tuple filters have been used.
    /// There are 128 such filters available.
    l34_5_tuple_filters: [bool; 128],
    /// The Rx queue that ethernet frames will be retrieved from in the next cycle 
    cur_rx_queue: u8,
}

/// A struct to store the rx descriptor queues for the ixgbe nic. 
/// It is separate from the main ixgbe struct so that multiple queues can be accessed in parallel
pub struct RxQueues {
    /// all the rx descriptor queues for this nic
    queue: Vec<MutexIrqSafe<RxQueueInfo>>,
    /// the number of rx descriptor queues for this nic
    num_queues: u8
}

/// A struct that holds all information for one receive queue. 
/// There should be one such object per queue
pub struct RxQueueInfo {
    /// the number of the queue, stored here for our convenience
    /// it should match the its index in the queue field of the RxQueue struct
    id: u8,
    /// Receive Descriptors
    rx_descs: BoxRefMut<MappedPages, [AdvancedRxDesc]>,
    /// Current Receive Descriptor index per queue
    rx_cur: u16,
    /// The list of rx buffers, in which the index in the vector corresponds to the index in `rx_descs`.
    rx_bufs_in_use: Vec<ReceiveBuffer>,
    /// The queue of received Ethernet frames, ready for consumption by a higher layer.
    /// Each frame is represented by a Vec<ReceiveBuffer>, because a single frame can span multiple receive buffers.    
    received_frames: VecDeque<ReceivedFrame>,
    /// the cpu which this queue is mapped to 
    /// this in itself guarantee anything but we use this value when setting the cpu id for interrupts and DCA
    cpu_id: u8,
}

/// A struct to store the tx descriptor queues for the ixgbe nic. 
/// It is separate from the main ixgbe struct so that multiple queues can be accessed in parallel
pub struct TxQueues {
    queue: Vec<MutexIrqSafe<TxQueueInfo>>,
    num_queues: u8
}

/// A struct that holds all information for a transmit queue. 
/// There should be one such object per queue
pub struct TxQueueInfo {
    /// the number of the queue, stored here for our convenience
    /// it should match the its index in the queue field of the TxQueue struct
    id: u8,
    /// Transmit Descriptors 
    tx_descs: BoxRefMut<MappedPages, [LegacyTxDesc]>,
    /// Current Transmit Descriptor index
    tx_cur: u16,
    /// the cpu which this queue is mapped to 
    /// this in itself guarantee anything but we use this value when setting the cpu id for interrupts and DCA
    cpu_id : u8
}

impl NicInit for IxgbeNic {}

impl NetworkInterfaceCard for IxgbeNic {

    fn send_packet(&mut self, transmit_buffer: TransmitBuffer) -> Result<(), &'static str>{
        // acquire the tx queue structure. The default queue is 0 since we haven't enabled multiple transmit queues.
        let qid = 0;
        let mut txq = get_ixgbe_tx_queues().ok_or("ixgbe: tx descriptors not initialized")?.queue[qid].lock();
        
        self.handle_transmit(&mut txq, transmit_buffer)?;

        // debug!("Packet is sent!");  
        Ok(())
    }

    fn get_received_frame(&mut self) -> Option<ReceivedFrame> {
        // the rx queue to retrieve ethernet frames from 
        let qid = self.cur_rx_queue as usize;
        // acquire the rx queue structure. 
        let mut rxq = match get_ixgbe_rx_queues() {
            Some(rx) => rx.queue[qid].lock(),
            None => return None
        };
        // Update the current rx queue so that the next time, another queue will be used in this function.
        // This way frames from all queues will be retrieved in round robin order 
        self.cur_rx_queue = (qid as u8 + 1) % IXGBE_NUM_RX_QUEUES;
        // return one frame from the queue's received frames
        rxq.received_frames.pop_front()
    }

    fn poll_receive(&mut self) -> Result<(), &'static str> {
        // Iterate through all the rx queues and collect their received frames
        for rxq in &(get_ixgbe_rx_queues().ok_or("ixgbe: rx descriptors not initalized")?.queue) {
            // let mut rxq = [qid as usize].lock();
            self.handle_receive(&mut rxq.lock())?;
        }
        Ok(())
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac_spoofed.unwrap_or(self.mac_hardware)
    }
}

/// functions that setup the NIC struct and handle the sending and receiving of packets
impl IxgbeNic {
    /// store required values from the devices PCI config space
    pub fn init(ixgbe_pci_dev: &PciDevice) -> Result<(), &'static str> {

        let bar0 = ixgbe_pci_dev.bars[0];
        // Determine the type from the base address register
        let bar_type = (bar0 as u8) & 0x01;    
        let mem_mapped = 0;

        // If the base address is not memory mapped then exit
        if bar_type != mem_mapped {
            error!("ixgbe::init: BAR0 is of I/O type");
            return Err("ixgbe::init: BAR0 is of I/O type")
        }

        // 16-byte aligned memory mapped base address
        let mem_base =  Self::determine_mem_base(ixgbe_pci_dev)?;

        // map the IntelIxgbeRegisters struct to the address found from the pci space
        let (mut mapped_registers, mem_base_v) = Self::mapped_reg(ixgbe_pci_dev, mem_base)?;

        // map the msi-x vector table to an address found from the pci space
        let mut vector_table = Self::mem_map_msix(ixgbe_pci_dev)?;

        // link initialization
        Self::start_link(&mut mapped_registers)?;

        // store the mac address of this device
        let mac_addr_hardware = Self::read_mac_address_from_nic(&mut mapped_registers);

        // initialize the buffer pool
        Self::init_rx_buf_pool(RX_BUFFER_POOL_SIZE, IXGBE_RX_BUFFER_SIZE_IN_BYTES, &RX_BUFFER_POOL)?;

        // create the rx desc queues and their packet buffers
        let (mut rx_descs, mut rx_buffers) = Self::rx_init(&mut mapped_registers)?;
        // create rx queue info struct for each rx_desc queue
        let mut rx_queues = Vec::new();
        let mut id = 0;
        while !rx_descs.is_empty() {
            let cpu_id = get_least_busy_core().ok_or("ixgbe::init: No core available")?;
            let rx_queue = RxQueueInfo {
                id: id,
                rx_descs: rx_descs.remove(0),
                rx_cur: 0,
                rx_bufs_in_use: rx_buffers.remove(0),  
                received_frames: VecDeque::new(),
                cpu_id : cpu_id,
            };
            rx_queues.push(MutexIrqSafe::new(rx_queue));
            id += 1;
        }
        // consolidate all the rx queues into one struct and store
        let ixgbe_rx_queues = RxQueues {
                                    queue: rx_queues,
                                    num_queues: IXGBE_NUM_RX_QUEUES
                                };
        let _ = IXGBE_RX_QUEUES.call_once(|| ixgbe_rx_queues);

        // create the tx descriptor queues and their packet buffers
        let mut tx_descs = Self::tx_init(&mut mapped_registers)?;
        // create tx queue info struct for each tx_desc queue
        let mut tx_queues = Vec::new();
        let mut id = 0;
        while !tx_descs.is_empty() {
            let cpu_id = get_least_busy_core().ok_or("ixgbe::init: No core available")?;
            let tx_queue = TxQueueInfo {
                id: id,
                tx_descs: tx_descs.remove(0),
                tx_cur: 0,
                cpu_id : cpu_id,
            };
            tx_queues.push(MutexIrqSafe::new(tx_queue));
            id += 1;
        }
        // consolidate all the tx queues into one struct and store
        let ixgbe_tx_queues = TxQueues {
                                    queue: tx_queues,
                                    num_queues: IXGBE_NUM_TX_QUEUES
                                };
        let _ = IXGBE_TX_QUEUES.call_once(|| ixgbe_tx_queues);

       
        // enable msi-x interrupts if required and return the assigned interrupt numbers
        let interrupt_num =
            if INTERRUPT_ENABLE {
                let interrupt_handlers: [HandlerFunc; NUM_MSI_VEC_ENABLED] = [ixgbe_handler_0];
                pci_enable_msix(ixgbe_pci_dev)?;
                pci_set_interrupt_disable_bit(ixgbe_pci_dev.bus, ixgbe_pci_dev.slot, ixgbe_pci_dev.func);
                Some(Self::enable_msix_interrupts(&mut mapped_registers, &mut vector_table, &interrupt_handlers)?)
            }
            else {
                None
            };

        // enable Receive Side Scaling if required
        if RSS_ENABLE {
            Self::setup_mrq(&mut mapped_registers)?;
        }

        let ixgbe_nic = IxgbeNic {
            bar_type: bar_type,
            mem_base: mem_base,
            mem_base_v: mem_base_v,
            eeprom_exists: false,
            interrupt_num: interrupt_num,
            mac_hardware: mac_addr_hardware,
            mac_spoofed: None,
            regs: mapped_registers,
            msix_vector_table: vector_table,
            l34_5_tuple_filters: [false; 128],
            cur_rx_queue: 0,
        };

        let nic_ref = IXGBE_NIC.call_once(|| MutexIrqSafe::new(ixgbe_nic));
        Ok(())       
    }

    /// Returns the memory mapped registers of the nic
    pub fn mapped_reg (dev: &PciDevice, mem_base: PhysicalAddress) -> Result<(BoxRefMut<MappedPages, IntelIxgbeRegisters>, VirtualAddress), &'static str> {
        let nic_mapped_page = Self::mem_map_reg(dev, mem_base)?;
        let mem_base_v = nic_mapped_page.start_address();
        let regs = BoxRefMut::new(Box::new(nic_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<IntelIxgbeRegisters>(0))?;
            
        debug!("Ixgbe status register: {:#X}", regs.status.read());
        Ok((regs, mem_base_v))
    }

    /// Returns the memory mapped msix vector table
    pub fn mem_map_msix(dev: &PciDevice) -> Result<BoxRefMut<MappedPages, MsixVectorTable>, &'static str> {
        // retreive the address in the pci config space for the msi-x capability
        let cap_addr = try!(pci_config_space(dev, MSIX_CAPABILITY).ok_or("ixgbe: device not have MSI-X capability"));
        // find the BAR used for msi-x
        let vector_table_offset = 4;
        let table_offset = pci_read_32(dev.bus, dev.slot, dev.func, cap_addr + vector_table_offset);
        let bar = table_offset & 0x7;
        let offset = table_offset >> 3;
        // find the memory base address and size of the area for the vector table
        let mem_base = PhysicalAddress::new((dev.bars[bar as usize] + offset) as usize)?;
        let mem_size_in_bytes = core::mem::size_of::<MsixVectorEntry>() * IXGBE_MAX_MSIX_VECTORS;

        debug!("msi-x vector table bar: {}, base_address: {:#X} and size: {} bytes", bar, mem_base, mem_size_in_bytes);

        let msix_mapped_pages = Self::mem_map(mem_base, mem_size_in_bytes)?;
        let vector_table = BoxRefMut::new(Box::new(msix_mapped_pages)).try_map_mut(|mp| mp.as_type_mut::<MsixVectorTable>(0))?;

        Ok(vector_table)
    }

    pub fn spoof_mac(&mut self, spoofed_mac_addr: [u8; 6]) {
        self.mac_spoofed = Some(spoofed_mac_addr);
    }

    /// Reads the actual MAC address burned into the NIC hardware.
    fn read_mac_address_from_nic(regs: &IntelIxgbeRegisters) -> [u8; 6] {
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

    /// acquires semaphore to synchronize between software and firmware (10.5.4)
    fn acquire_semaphore(regs: &mut IntelIxgbeRegisters) -> Result<bool, &'static str> {
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

        // clear sw sempahore bits if sw malfunction
        if timer_expired_smbi {
            sw_fw_sync_smbits &= !(SW_FW_SYNC_SMBITS_SW);
        }

        // clear fw semaphore bits if fw malfunction
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

    /// release the semaphore synchronizing between software and firmware
    fn release_semaphore(regs: &mut IntelIxgbeRegisters) -> Result<(), &'static str> {
        // clear bit of released resource
        let sw_fw_sync = regs.sw_fw_sync.read() & !(SW_FW_SYNC_SW_MAC);
        regs.sw_fw_sync.write(sw_fw_sync);

        // release semaphore
        let _swsm = regs.swsm.read() & !(SWSM_SMBI) & !(SWSM_SWESMBI);

        Ok(())
    }

    /// software reset of NIC to get it running
    fn start_link (mut regs: &mut IntelIxgbeRegisters) -> Result<(), &'static str>{
        //disable interrupts: write to EIMC registers, 1 in b30-b0, b31 is reserved
        regs.eimc.write(DISABLE_INTERRUPTS);

        // master disable algorithm (sec 5.2.5.3.2)
        // global reset = sw reset + link reset 
        let val = regs.ctrl.read();
        regs.ctrl.write(val|CTRL_RST|CTRL_LRST);

        //wait 10 ms
        let wait_time = 10_000;
        let _ =pit_clock::pit_wait(wait_time);

        //disable flow control.. write 0 TO FCTTV, FCRTL, FCRTH, FCRTV and FCCFG
        for fcttv in regs.fcttv.reg.iter_mut() {
            fcttv.write(0);
        }

        for fcrtl in regs.fcrtl.reg.iter_mut() {
            fcrtl.write(0);
        }

        for fcrth in regs.fcrth.reg.iter_mut() {
            fcrth.write(0);
        }

        regs.fcrtv.write(0);
        regs.fccfg.write(0);

        //disable interrupts
        regs.eims.write(DISABLE_INTERRUPTS);

        //wait for eeprom auto read completion?

        //read MAC address
        debug!("MAC address low: {:#X}", regs.ral.read());
        debug!("MAC address high: {:#X}", regs.rah.read() & 0xFFFF);

        //wait for dma initialization done (RDRXCTL.DMAIDONE)
        let mut val = regs.rdrxctl.read();
        let dmaidone_bit = 1 << 3;
        while val & dmaidone_bit != dmaidone_bit {
            val = regs.rdrxctl.read();
        }
        debug!("RDRXCTL: {:#X}", regs.rdrxctl.read()); 

        while Self::acquire_semaphore(&mut regs)? {
            //wait 10 ms
            let _ =pit_clock::pit_wait(wait_time);
        }
        debug!("IXGBE: Semaphore acquired!");

        //setup PHY and the link
        debug!("AUTOC: {:#X}", regs.autoc.read()); 
        if IXGBE_10GB_LINK {
            let mut val = regs.autoc.read();
            regs.autoc.write(val | AUTOC_LMS_10_GBE_S); // value should be 0xC09C_6004

            let mut val = regs.autoc2.read();
            regs.autoc2.write(val | AUTOC2_10G_PMA_PMD_S_SFI); // value should be 0xA_0000
        }
        else {
            let mut val = regs.autoc.read();
            val = (val & !(AUTOC_LMS_1_GB) & !(AUTOC_1G_PMA_PMD)) | AUTOC_FLU;
            regs.autoc.write(val);
        }

        let val = regs.autoc.read();
        regs.autoc.write(val|AUTOC_RESTART_AN); 

        Self::release_semaphore(&mut regs)?;        

        debug!("STATUS: {:#X}", regs.status.read()); 
        debug!("CTRL: {:#X}", regs.ctrl.read());
        debug!("LINKS: {:#X}", regs.links.read()); //b7 and b30 should be 1 for link up 
        debug!("AUTOC: {:#X}", regs.autoc.read()); 
        debug!("AUTOC2: {:#X}", regs.autoc2.read()); 

        Ok(())
    }

    /// Initialize the array of receive descriptors and their corresponding receive buffers,
    /// and returns a tuple including both of them for all rx queues in use.
    pub fn rx_init(regs: &mut IntelIxgbeRegisters) -> Result<(Vec<BoxRefMut<MappedPages, [AdvancedRxDesc]>>, Vec<Vec<ReceiveBuffer>>), &'static str>  {

        let mut rx_descs_all_queues = Vec::new();
        let mut rx_bufs_in_use_all_queues = Vec::new();
        
        for queue in 0..IXGBE_NUM_RX_QUEUES {

            // choose which set of rx queue registers needs to be accessed for this queue
            // because rx registers are divided into 2 sets of 64 queues in memory
            let (rx_queue_regs, qid) = 
                if queue < 64 {
                    (&mut regs.rx_regs1, queue)
                }
                else {
                    (&mut regs.rx_regs2, queue - 64)
                };

            let (rx_descs, rx_bufs_in_use) = Self::init_rx_queue(IXGBE_NUM_RX_DESC, &RX_BUFFER_POOL, IXGBE_RX_BUFFER_SIZE_IN_BYTES as usize, &mut rx_queue_regs.rx_queue[qid as usize].rdbal, 
                                            &mut rx_queue_regs.rx_queue[qid as usize].rdbah, &mut rx_queue_regs.rx_queue[qid as usize].rdlen, &mut rx_queue_regs.rx_queue[qid as usize].rdh,
                                            &mut rx_queue_regs.rx_queue[qid as usize].rdt)?;          
            

            //set the size of the packet buffers and the descriptor format used
            let mut val = rx_queue_regs.rx_queue[qid as usize].srrctl.read();
            val.set_bits(0..4, BSIZEPACKET_8K);
            val.set_bits(8..13, BSIZEHEADER_256B);
            val.set_bits(25..27, DESCTYPE_ADV_1BUFFER);
            rx_queue_regs.rx_queue[qid as usize].srrctl.write(val);

            //enable the rx queue
            let mut val = rx_queue_regs.rx_queue[qid as usize].rxdctl.read();
            val.set_bit(25, RX_Q_ENABLE);
            rx_queue_regs.rx_queue[qid as usize].rxdctl.write(val);

            //make sure queue is enabled
            while rx_queue_regs.rx_queue[qid as usize].rxdctl.read().get_bit(25) != RX_Q_ENABLE {}
            
            // Write the tail index.
            // Note that the 82599 datasheet (section 8.2.3.8.5) states that we should set the RDT (tail index) to the index *beyond* the last receive descriptor, 
            // but we set it to the last receive descriptor for the same reason as the e1000 driver
            rx_queue_regs.rx_queue[qid as usize].rdt.write((IXGBE_NUM_RX_DESC - 1) as u32);

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

    /// enable multiple receive queues
    /// Part of queue initialization is done in the rx_init function per queue
    pub fn setup_mrq(regs: &mut IntelIxgbeRegisters) -> Result<(), &'static str>{
        // enable RSS writeback in the header field of the receive descriptor
        regs.rxcsum.write(RXCSUM_PCSD);
        
        // enable RSS and set fields that will be used by hash function
        regs.mrqc.write(MRQC_MRQE_RSS | MRQC_UDPIPV4 ); 

        //set the random keys for the hash function
        let seed = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
        let mut rng = SmallRng::seed_from_u64(seed);
        let num_rssrk_reg = 10;
        for i in 0..num_rssrk_reg {
            regs.rssrk.reg[i].write(rng.next_u32());
        }

        // Initialize the RSS redirection table
        // each reta register has 4 redirection entries
        // since mapping to queues is random and based on a hash, we randomly assign 1 queue to each reta register
        let num_reta_regs = 32;
        let mut qid = 0;
        for i in 0..num_reta_regs {
            //set 4 entries to the same queue number
            let val = qid << RETA_ENTRY_0_OFFSET | qid << RETA_ENTRY_1_OFFSET | qid << RETA_ENTRY_2_OFFSET | qid << RETA_ENTRY_3_OFFSET;
            regs.reta.reg[i].write(val);

            // next 4 entries will be assigned to the next queue
            qid += 1;
            if qid >= IXGBE_NUM_RX_QUEUES as u32 {
                qid = 0;
            }
        }
        Ok(())
    }
    
 
    /// Enables Direct Cache Access for the device
    /// TODO: need to see if to allow DCA from this device, the identification number has to be programmed into the chipset register
    fn enable_dca(regs: &mut IntelIxgbeRegisters) -> Result<(), &'static str> {
        // Enable DCA tagging, which writes the cpu id to the PCIe Transaction Layer Packets (TLP)
        // There are 2 version of DCA that are mentioned, legacy and 1.0
        // We always enable 1.0 since 1. currently haven't found additional info online and 2. the linux driver always enables 1.0 
        regs.dca_ctrl.write(DCA_MODE_2 | DCA_ENABLE);
        Self::enable_rx_dca(regs)  
    }

    /// Sets up DCA for the rx queues that have been enabled
    /// you can optionally choose to have the descriptor, header and payload copied to the cache for each received packet
    fn enable_rx_dca(regs: &mut IntelIxgbeRegisters) -> Result<(), &'static str>{
        let rxq = get_ixgbe_rx_queues().ok_or("ixgbe: rx descriptors not initialized")?;
        let num_queues_enabled = rxq.num_queues as usize;

        for i in 0..num_queues_enabled {
            // retrieve the dca rx control register depending on the queue number
            // there are 64 contiguous queues at 2 different places in memory
            let rxctrl =  
                if i < 64 {
                    &mut regs.rx_regs1.rx_queue[i].dca_rxctrl
                }
                else {
                    &mut regs.rx_regs2.rx_queue[i-64].dca_rxctrl                    
                };
            
            // the cpu id will tell which cache the data will need to be written to
            let cpu_id = rxq.queue[i].lock().cpu_id as u32;
            
            // currently allowing only write of rx descriptors to cache since packet payloads are very large
            // A note from linux ixgbe driver: 
            // " We can enable relaxed ordering for reads, but not writes when
            //   DCA is enabled.  This is due to a known issue in some chipsets
            //   which will cause the DCA tag to be cleared."
            rxctrl.write(RX_DESC_DCA_ENABLE | RX_DESC_R_RELAX_ORDER_EN | (cpu_id << DCA_CPUID_SHIFT));
        }
        Ok(())
    }

    /// sets the L3/L4 5-tuple filter which can do an exact match of the packets with the filter and send to chosen rx queue (7.1.2.5)
    /// takes as input: source and destination ip addresses (only ipv4), source and destination TCP/UDP/SCTP ports and protocol  
    /// for the protocol : tcp = 0, udp = 1, sctp = 2, other = 3
    /// the priority can be from 0 (lowest) to 7 (highest)
    /// There are up to 128 such filters, if you need more than will have to enable Flow Director filters
    fn set_5_tuple_filter(&mut self, source_ip: [u8;4], dest_ip: [u8;4], source_port: u16, dest_port: u16, protocol: u8, priority: u8, rx_queue: u8) -> Result<(), &'static str> {
  
        let enabled_filters = &mut self.l34_5_tuple_filters;

        // find a free filter
        let filter_num = enabled_filters.iter().position(|&r| r == false).ok_or("Ixgbe: No filter available")?;

        // IP addresses are written to the registers in big endian form (LSB is first on wire)
        // set the source ip address for the filter
        self.regs.saqf.reg[filter_num].write(((source_ip[0] as u32) << 24) | ((source_ip[1] as u32) << 16) | ((source_ip[2] as u32) << 8) | (source_ip[3] as u32));
        // set the destination ip address for the filter        
        self.regs.daqf.reg[filter_num].write(((dest_ip[0] as u32) << 24) | ((dest_ip[1] as u32) << 16) | ((dest_ip[2] as u32) << 8) | (dest_ip[3] as u32));
        // set the source and destination ports for the filter        
        self.regs.sdpqf.reg[filter_num].write(((source_port as u32) << SPDQF_SOURCE_SHIFT) | ((dest_port as u32) << SPDQF_DEST_SHIFT));

        // set up the parameters of the filter
        let filter_protocol = protocol as u32 & FTQF_PROTOCOL;
        let filter_priority = (priority as u32 & FTQF_PRIORITY) << FTQF_PRIORITY_SHIFT;
        self.regs.ftqf.reg[filter_num].write(filter_protocol | filter_priority | FTQF_Q_ENABLE);

        //set the rx queue that the packets for this filter should be sent to
        self.regs.l34timir.reg[filter_num].write(L34TIMIR_BYPASS_SIZE_CHECK | L34TIMIR_RESERVED | ((rx_queue as u32) << L34TIMIR_RX_Q_SHIFT));

        Ok(())
    }

    /// Initialize the array of tramsmit descriptors and return them.
    fn tx_init(regs: &mut IntelIxgbeRegisters) -> Result<Vec<BoxRefMut<MappedPages, [LegacyTxDesc]>>, &'static str>   {
        //disable transmission
        let val = regs.dmatxctl.read();
        regs.dmatxctl.write(val & !TE); 

        //let val = self.read_command(REG_RTTDCS);
        //self.write_command(REG_RTTDCS,val | 1<<6 ); // set b6 to 1

        let qid = 0;

        let tx_descs = Self::init_tx_queue(IXGBE_NUM_TX_DESC, &mut regs.tx_regs.tx_queue[qid as usize].tdbal, &mut regs.tx_regs.tx_queue[qid as usize].tdbah, 
                        &mut regs.tx_regs.tx_queue[qid as usize].tdlen, &mut regs.tx_regs.tx_queue[qid as usize].tdh, &mut regs.tx_regs.tx_queue[qid as usize].tdt)?;
        
        // enable transmit operation
        let val = regs.dmatxctl.read();
        regs.dmatxctl.write(val | TE); 

        //let val = self.read_command(REG_RTTDCS);
        //self.write_command(REG_RTTDCS,val & 0xFFFF_FFBF); // set b6 to 0
        
        //enable tx queue
        let mut val = regs.tx_regs.tx_queue[qid].txdctl.read();
        val.set_bit(25, TX_Q_ENABLE);
        regs.tx_regs.tx_queue[qid].txdctl.write(val); 

        //make sure queue is enabled
        while regs.tx_regs.tx_queue[qid].txdctl.read().get_bit(25) != TX_Q_ENABLE {} 

        Ok(vec![tx_descs])
    }  

    /// enable interrupts for msi-x mode
    /// currently all the msi vectors are for packet reception, one msi vector per queue
    fn enable_msix_interrupts(regs: &mut IntelIxgbeRegisters, vector_table: &mut MsixVectorTable, interrupt_handlers: &[HandlerFunc; NUM_MSI_VEC_ENABLED]) -> Result<[u8; NUM_MSI_VEC_ENABLED], &'static str> {
        // set IVAR reg to enable interrupts for different queues
        // each IVAR register controls 2 RX and 2 TX queues
        let num_queues = IXGBE_NUM_RX_QUEUES as usize;
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
            debug!("IVAR: {:#X}", regs.ivar.reg[queue / queues_per_ivar_reg].read());
        }
        
        //enable clear on read of EICR and MSI-X mode
        let val = regs.gpie.read();
        regs.gpie.write(val | GPIE_EIMEN | GPIE_MULTIPLE_MSIX | GPIE_PBA_SUPPORT); 
        debug!("GPIE: {:#X}", regs.gpie.read());

        //set eims to enable required interrupt
        regs.eims.write(0xFFFF); 
        debug!("EIMS: {:#X}", regs.eims.read());

        //enable auto-clear of receive interrupts 
        regs.eiac.write(EIAC_RTXQ_AUTO_CLEAR);

        //clears eicr by writing 1 to clear old interrupt causes?
        let val = regs.eicr.read();
        debug!("EICR: {:#X}", val);

        // // set the throttling time for each interrupt
        // // minimum interrupt interval specified in 2us units
        for i in 0..NUM_MSI_VEC_ENABLED {
            regs.eitr.reg[i].write(1 << EITR_ITR_INTERVAL_SHIFT);
        }

        let mut interrupt_nums = [0; NUM_MSI_VEC_ENABLED];
        // retrieve rx descriptors to know which cpu to send the interrupt to
        let rxq = get_ixgbe_rx_queues().ok_or("ixgbe: rx descriptors not initialized")?;

        // Initialize msi vectors
        for i in 0..NUM_MSI_VEC_ENABLED{
            // register an interrupt handler and get a free interrupt number that can be used for the msix int
            interrupt_nums[i] = register_msi_interrupt(interrupt_handlers[i])?;

            // find core to redirect interrupt to
            // we assume that the number of msi vectors are <= the number of rx queues
            let cpu = rxq.queue[i].lock().cpu_id;
            let core_id = cpu as u32;

            //allocate an interrupt to msix vector
            vector_table.msi_vector[i].vector_control.write(MSIX_UNMASK_INT);
            let lower_addr = vector_table.msi_vector[i].msg_lower_addr.read();
            vector_table.msi_vector[i].msg_lower_addr.write((lower_addr & !MSIX_ADDRESS_BITS) | MSIX_INTERRUPT_REGION | (core_id << MSIX_DEST_ID_SHIFT)); 
            vector_table.msi_vector[i].msg_data.write(interrupt_nums[i] as u32);
            debug!("Created MSI vector: control: {}, core: {}, int: {}", vector_table.msi_vector[i].vector_control.read(), core_id, interrupt_nums[i]);
        }

        Ok(interrupt_nums)
    }

    // reads status and clears interrupt
    fn clear_interrupt_status(&self) -> u32 {
        self.regs.eicr.read()
    }

    /// write to an NIC register
    /// p_address is register offset
    pub fn write_command(&self,p_address:u32, p_value:u32){
        unsafe { write_volatile((self.mem_base_v.value() as u32 + p_address) as *mut u32, p_value) }
    }

    /// read from an NIC register
    /// p_address is register offset
    fn read_command(&self,p_address:u32) -> u32 {
        unsafe { read_volatile((self.mem_base_v.value() as u32 + p_address) as *const u32) }

    }

    /// Returns all the receive buffers in one packet
    /// Called for individual queues
    pub fn handle_receive(&mut self, rxq: &mut RxQueueInfo) -> Result<(), &'static str> {

        // choose which set of rx queue registers needs to be accessed for this queue
        // because rx registers are divided into 2 sets of 64 queues in memory
        let (rxq_regs, qid) =
            if rxq.id < 64 {
                (&mut self.regs.rx_regs1, rxq.id)
            }
            else {
                (&mut self.regs.rx_regs2, rxq.id - 64)
            };

        Self::collect_packets(&mut rxq.rx_cur, &mut rxq.rx_descs, IXGBE_NUM_RX_DESC as u16, &RX_BUFFER_POOL, IXGBE_RX_BUFFER_SIZE_IN_BYTES, 
            &mut rxq.rx_bufs_in_use, &mut rxq.received_frames, &mut rxq_regs.rx_queue[qid as usize].rdt)
    }

    fn handle_transmit(&mut self, txq: &mut TxQueueInfo, transmit_buffer: TransmitBuffer) -> Result<(), &'static str> {
        // update the descriptors and tdt register
        Self::send(&mut txq.tx_cur, IXGBE_NUM_TX_DESC as u16, &mut txq.tx_descs, &mut self.regs.tx_regs.tx_queue[txq.id as usize].tdt, transmit_buffer);
        Ok(())
    }

    fn handle_rx_interrupt(&mut self, qid: u8) {
        // let me = get_my_apic_id().unwrap();

        if let Some(ref rxqs) = get_ixgbe_rx_queues() {
            let mut rxq = rxqs.queue[qid as usize].lock();
            if let Err(e) = self.handle_receive(&mut rxq) {
                error!("handle_rx_interrupt(): error handling interrupt: {:?}", e);
            }
        } else {
            error!("BUG: handle_rx_interrupt(): Ixgbe Rx descriptors haven't been initialized!");
        };        
    }

}

/// A helper function to poll the nic receive queues
pub fn rx_poll_mq(_: Option<u64>) -> Result<(), &'static str> {
    loop {
        let mut nic = IXGBE_NIC.try().ok_or("ixgbe NIC hasn't been initialized yet")?.lock();

        for qid in 0..IXGBE_NUM_RX_QUEUES {
            let mut rxq = get_ixgbe_rx_queues().ok_or("ixgbe: rx descriptors not initalized")?.queue[qid as usize].lock();
            nic.handle_receive(&mut rxq)?;                    
        }        
    }
}

/// A generic interrupt handler that can be used for packet reception interrupts for ay queue
fn rx_interrupt_handler(qid: u8) -> u8 {
    let interrupt_num = 
        if let Some(ref ixgbe_nic_ref) = IXGBE_NIC.try() {
            let mut ixgbe_nic = ixgbe_nic_ref.lock();
            ixgbe_nic.handle_rx_interrupt(qid);
            ixgbe_nic.interrupt_num.unwrap()[qid as usize]
        } else {
            error!("BUG: ixgbe_handler_{}(): IXGBE NIC hasn't yet been initialized!", qid);
            0
        };
    
    interrupt_num
}

/// The interrupt handler for rx queue 0
extern "x86-interrupt" fn ixgbe_handler_0(_stack_frame: &mut ExceptionStackFrame) {
    eoi(Some(rx_interrupt_handler(0)));
}










