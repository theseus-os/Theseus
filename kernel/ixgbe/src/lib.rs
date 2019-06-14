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

use spin::Once;
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use irq_safety::{MutexIrqSafe, RwLockIrqSafe};
use alloc::boxed::Box;
use memory::{PhysicalAddress, VirtualAddress, MappedPages};
use pci::{PciDevice, pci_read_32, pci_set_interrupt_disable_bit, pci_enable_msix, pci_config_space, MSIX_CAPABILITY};
use registers::*;
use bit_field::BitField;
use interrupts::{eoi,register_msi_interrupt};
use x86_64::structures::idt::{ExceptionStackFrame, HandlerFunc};
use hpet::get_hpet;
use network_interface_card::{
    {NetworkInterfaceCard, TransmitBuffer, ReceiveBuffer, ReceivedFrame},
    intel_ethernet::{NicInit, AdvancedRxDesc, LegacyTxDesc, TxDescriptor, RxDescriptor, RxQueue, TxQueue},
};
use owning_ref::BoxRefMut;
use rand::{
    SeedableRng,
    RngCore,
    rngs::SmallRng
};
use runqueue::get_least_busy_core;


/** Default configuration at time of initialization of NIC **/
/// If interrupts are enabled for packet reception
const INTERRUPT_ENABLE:                     bool    = false;
/// The number of MSI vectors enabled for the NIC, with the maximum for the 82599 being 64.
/// This number is only relevant if interrupts are enabled.
const NUM_MSI_VEC_ENABLED:                  u8   = IXGBE_NUM_RX_QUEUES;
/// If link uses 10GB SFP modules
const IXGBE_10GB_LINK:                      bool    = true;
/// If link uses 1GB SFP modules
const IXGBE_1GB_LINK:                       bool    = !(IXGBE_10GB_LINK);
/// The number of receive descriptors per queue
const IXGBE_NUM_RX_DESC:                    usize   = 256;
/// The number of transmit descriptors per queue
const IXGBE_NUM_TX_DESC:                    usize   = 8;
/// If receive side scaling (where incoming packets are sent to different queues depending on a hash) is enabled.
const RSS_ENABLE:                           bool    = false;
/// If the 5-tuple L3/L4 filters to send packets to different queues are enabled.
const FILTER_ENABLE:                        bool    = false;
/// The number of receive queues that are enabled.
/// It can be a a maximum of 16 since we are using only the physical functions.
/// If RSS or filters are not enabled, this should be 1.
const IXGBE_NUM_RX_QUEUES:                  u8      = 1;
/// The number of transmit queues that are enabled. 
/// Support for multiple Tx queues hasn't been added so this should remain 1.
const IXGBE_NUM_TX_QUEUES:                  u8      = 1;
/// Size of the Rx packet buffers
const IXGBE_RX_BUFFER_SIZE_IN_BYTES:        u16     = 8192;


/// The single instance of the 82599 NIC.
pub static IXGBE_NIC: Once<RwLockIrqSafe<IxgbeNic>> = Once::new();

/// Returns a reference to the IxgbeNic wrapped in a RwLockIrqSafe,
/// if it exists and has been initialized.
pub fn get_ixgbe_nic() -> Option<&'static RwLockIrqSafe<IxgbeNic>> {
    IXGBE_NIC.try()
}


/// How many ReceiveBuffers are preallocated for this driver to use. 
const RX_BUFFER_POOL_SIZE:                  usize = IXGBE_NUM_RX_QUEUES as usize * IXGBE_NUM_RX_DESC * 2; 

lazy_static! {
    /// The pool of pre-allocated receive buffers that are used by the IXGBE NIC
    /// and temporarily given to higher layers in the networking stack.
    static ref RX_BUFFER_POOL: mpmc::Queue<ReceiveBuffer> = mpmc::Queue::with_capacity(RX_BUFFER_POOL_SIZE);
}

/// A struct representing an ixgbe network interface card
pub struct IxgbeNic {
    /// Type of Base Address Register 0,
    /// if it's memory mapped or I/O.
    bar_type: u8,
    /// MMIO Base Address     
    mem_base: PhysicalAddress,
    /// MMIO Base virtual address
    mem_base_v: VirtualAddress, 
    /// Interrupt number for each msi vector
    interrupt_num: Option<[u8; NUM_MSI_VEC_ENABLED as usize]>,
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
    /// The number of rx queues enabled
    num_rx_queues: u8,
    /// Vector of all the rx queues
    rx_queues: Vec<MutexIrqSafe<RxQueue<AdvancedRxDesc>>>,
    /// The number of tx queues enabled
    num_tx_queues: u8,
    /// Vector of all the tx queues
    tx_queues: Vec<MutexIrqSafe<TxQueue<LegacyTxDesc>>>
}



// A trait which contains common initialization procedures for Intel NICs
impl NicInit for IxgbeNic {}

// A trait which contains common functionalities for a NIC
impl NetworkInterfaceCard for IxgbeNic {

    fn send_packet(&self, transmit_buffer: TransmitBuffer) -> Result<(), &'static str>{
        // the default queue is 0 since we haven't enabled multiple transmit queues.
        let qid = 0;
        // acquire the tx queue structure.
        let mut txq = self.tx_queues[qid].lock();
        self.handle_transmit(&mut txq, transmit_buffer)?;

        Ok(())
    }

    // this function has only been tested with 1 Rx queue and is meant to be used with the smoltcp stack.
    fn get_received_frame(&mut self) -> Option<ReceivedFrame> {
        // the rx queue to retrieve ethernet frames from 
        let qid = self.cur_rx_queue as usize;
        // acquire the rx queue structure. 
        let mut rxq =self.rx_queues[qid].lock();
        // update the current rx queue so that in the next cycle, frames will be returned from the next queue.
        // this way frames from all queues will be retrieved in round robin order.
        self.cur_rx_queue = (qid as u8 + 1) % IXGBE_NUM_RX_QUEUES;
        // return one frame from the queue's received frames
        rxq.received_frames.pop_front()
    }

    fn poll_receive(&self) -> Result<(), &'static str> {
        // iterate through all the rx queues and collect their received frames
        for rxq in &self.rx_queues {
            self.handle_receive(&mut rxq.lock())?;
        }
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
        // create the vec of rx queues
        let mut rx_queues = Vec::new();
        let mut id = 0;
        while !rx_descs.is_empty() {
            let cpu_id = get_least_busy_core().ok_or("ixgbe::init: No core available")?;

            // find the offset in the memory mapped registers that this queue's rdt register is at
            let (rdt_offset, qid) =
                if id < 64 {
                    (RDT_1, id)
                }
                else {
                    (RDT_2, id-64)
                };

            let rx_queue = RxQueue {
                id: id,
                rx_descs: rx_descs.remove(0),
                rx_cur: 0,
                rx_bufs_in_use: rx_buffers.remove(0),  
                received_frames: VecDeque::new(),
                cpu_id : cpu_id,
                rdt_addr: VirtualAddress::new(mem_base_v.value() + rdt_offset + (qid as usize * RDT_DIST))?,
            };
            rx_queues.push(MutexIrqSafe::new(rx_queue));
            id += 1;
        }


        // create the tx descriptor queues
        let mut tx_descs = Self::tx_init(&mut mapped_registers)?;
        // create the vec of tx queues
        let mut tx_queues = Vec::new();
        let mut id = 0;
        while !tx_descs.is_empty() {
            let cpu_id = get_least_busy_core().ok_or("ixgbe::init: No core available")?;
            let tx_queue = TxQueue {
                id: id,
                tx_descs: tx_descs.remove(0),
                tx_cur: 0,
                cpu_id : cpu_id,
                tdt_addr: VirtualAddress::new(mem_base_v.value() + TDT + (id as usize * TDT_DIST))?,
            };
            tx_queues.push(MutexIrqSafe::new(tx_queue));
            id += 1;
        }

       
        // enable msi-x interrupts if required and return the assigned interrupt numbers
        let interrupt_num =
            if INTERRUPT_ENABLE {
                // the collection of interrupt handlers for the receive queues. There might be a better way to do this 
                // but right now we have to explicitly create each interrupt handler and add it here.
                let interrupt_handlers: [HandlerFunc; NUM_MSI_VEC_ENABLED as usize] = [ixgbe_handler_0];
                pci_enable_msix(ixgbe_pci_dev)?;
                pci_set_interrupt_disable_bit(ixgbe_pci_dev.bus, ixgbe_pci_dev.slot, ixgbe_pci_dev.func);
                Some(Self::enable_msix_interrupts(&mut mapped_registers, &mut rx_queues, &mut vector_table, &interrupt_handlers)?)
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
            interrupt_num: interrupt_num,
            mac_hardware: mac_addr_hardware,
            mac_spoofed: None,
            regs: mapped_registers,
            msix_vector_table: vector_table,
            l34_5_tuple_filters: [false; 128],
            cur_rx_queue: 0,
            num_rx_queues: IXGBE_NUM_RX_QUEUES,
            rx_queues: rx_queues,
            num_tx_queues: IXGBE_NUM_TX_QUEUES,
            tx_queues: tx_queues
        };

        IXGBE_NIC.call_once(|| RwLockIrqSafe::new(ixgbe_nic));
        Ok(())       
    }

    /// Returns the memory mapped registers of the nic
    pub fn mapped_reg (dev: &PciDevice, mem_base: PhysicalAddress) -> Result<(BoxRefMut<MappedPages, IntelIxgbeRegisters>, VirtualAddress), &'static str> {
        let nic_mapped_page = Self::mem_map_reg(dev, mem_base)?;
        let mem_base_v = nic_mapped_page.start_address();
        let regs = BoxRefMut::new(Box::new(nic_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<IntelIxgbeRegisters>(0))?;
            
        Ok((regs, mem_base_v))
    }

    /// Returns the memory mapped msix vector table
    pub fn mem_map_msix(dev: &PciDevice) -> Result<BoxRefMut<MappedPages, MsixVectorTable>, &'static str> {
        // retreive the address in the pci config space for the msi-x capability
        let cap_addr = try!(pci_config_space(dev, MSIX_CAPABILITY).ok_or("ixgbe: device does not have MSI-X capability"));
        // find the BAR used for msi-x
        let vector_table_offset = 4;
        let table_offset = pci_read_32(dev.bus, dev.slot, dev.func, cap_addr + vector_table_offset);
        let bar = table_offset & 0x7;
        let offset = table_offset >> 3;
        // find the memory base address and size of the area for the vector table
        let mem_base = PhysicalAddress::new((dev.bars[bar as usize] + offset) as usize)?;
        let mem_size_in_bytes = core::mem::size_of::<MsixVectorEntry>() * IXGBE_MAX_MSIX_VECTORS;

        // debug!("msi-x vector table bar: {}, base_address: {:#X} and size: {} bytes", bar, mem_base, mem_size_in_bytes);

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

    /// Acquires semaphore to synchronize between software and firmware (10.5.4)
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
    fn release_semaphore(regs: &mut IntelIxgbeRegisters) -> Result<(), &'static str> {
        // clear bit of released resource
        let sw_fw_sync = regs.sw_fw_sync.read() & !(SW_FW_SYNC_SW_MAC);
        regs.sw_fw_sync.write(sw_fw_sync);

        // release semaphore
        let _swsm = regs.swsm.read() & !(SWSM_SMBI) & !(SWSM_SWESMBI);

        Ok(())
    }

    /// Software reset of NIC to get it running
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
        debug!("Ixgbe: MAC address low: {:#X}", regs.ral.read());
        debug!("Ixgbe: MAC address high: {:#X}", regs.rah.read() & 0xFFFF);

        //wait for dma initialization done (RDRXCTL.DMAIDONE)
        let mut val = regs.rdrxctl.read();
        let dmaidone_bit = 1 << 3;
        while val & dmaidone_bit != dmaidone_bit {
            val = regs.rdrxctl.read();
        }

        while Self::acquire_semaphore(&mut regs)? {
            //wait 10 ms
            let _ =pit_clock::pit_wait(wait_time);
        }

        //setup PHY and the link 
        if IXGBE_10GB_LINK {
            let val = regs.autoc.read();
            regs.autoc.write(val | AUTOC_LMS_10_GBE_S); // value should be 0xC09C_6004

            let val = regs.autoc2.read();
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

        // debug!("STATUS: {:#X}", regs.status.read()); 
        // debug!("CTRL: {:#X}", regs.ctrl.read());
        // debug!("LINKS: {:#X}", regs.links.read()); //b7 and b30 should be 1 for link up 
        // debug!("AUTOC: {:#X}", regs.autoc.read()); 
        // debug!("AUTOC2: {:#X}", regs.autoc2.read()); 

        Ok(())
    }

    /// Initializes the array of receive descriptors and their corresponding receive buffers,
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

            // get the queue of rx descriptors and their corresponding rx buffers
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

    /// Enable multiple receive queues with RSS.
    /// Part of queue initialization is done in the rx_init function.
    pub fn setup_mrq(regs: &mut IntelIxgbeRegisters) -> Result<(), &'static str>{
        // enable RSS writeback in the header field of the receive descriptor
        regs.rxcsum.write(RXCSUM_PCSD);
        
        // enable RSS and set fields that will be used by hash function
        // right now we're using the udp port and ipv4 address.
        regs.mrqc.write(MRQC_MRQE_RSS | MRQC_UDPIPV4 ); 

        //set the random keys for the hash function
        let seed = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
        let mut rng = SmallRng::seed_from_u64(seed);
        for rssrk in regs.rssrk.reg.iter_mut() {
            rssrk.write(rng.next_u32());
        }

        // Initialize the RSS redirection table
        // each reta register has 4 redirection entries
        // since mapping to queues is random and based on a hash, we randomly assign 1 queue to each reta register
        let mut qid = 0;
        for reta in regs.reta.reg.iter_mut() {
            //set 4 entries to the same queue number
            let val = qid << RETA_ENTRY_0_OFFSET | qid << RETA_ENTRY_1_OFFSET | qid << RETA_ENTRY_2_OFFSET | qid << RETA_ENTRY_3_OFFSET;
            reta.write(val);

            // next 4 entries will be assigned to the next queue
            qid = (qid + 1) % IXGBE_NUM_RX_QUEUES as u32;
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
    fn set_5_tuple_filter(&mut self, source_ip: [u8;4], dest_ip: [u8;4], source_port: u16, dest_port: u16, protocol: u8, priority: u8, qid: u8) -> Result<(), &'static str> {
  
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
        self.regs.l34timir.reg[filter_num].write(L34TIMIR_BYPASS_SIZE_CHECK | L34TIMIR_RESERVED | ((qid as u32) << L34TIMIR_RX_Q_SHIFT));

        //mark the filter as used
        enabled_filters[filter_num] = true;

        Ok(())
    }

    /// Initialize the array of tramsmit descriptors and return them.
    fn tx_init(regs: &mut IntelIxgbeRegisters) -> Result<Vec<BoxRefMut<MappedPages, [LegacyTxDesc]>>, &'static str>   {
        //disable transmission
        let val = regs.dmatxctl.read();
        regs.dmatxctl.write(val & !TE); 

        // only initialize 1 queue for now
        let qid = 0;

        let tx_descs = Self::init_tx_queue(IXGBE_NUM_TX_DESC, &mut regs.tx_regs.tx_queue[qid as usize].tdbal, &mut regs.tx_regs.tx_queue[qid as usize].tdbah, 
                        &mut regs.tx_regs.tx_queue[qid as usize].tdlen, &mut regs.tx_regs.tx_queue[qid as usize].tdh, &mut regs.tx_regs.tx_queue[qid as usize].tdt)?;
        
        // enable transmit operation
        let val = regs.dmatxctl.read();
        regs.dmatxctl.write(val | TE); 
        
        //enable tx queue
        let mut val = regs.tx_regs.tx_queue[qid].txdctl.read();
        val.set_bit(25, TX_Q_ENABLE);
        regs.tx_regs.tx_queue[qid].txdctl.write(val); 

        //make sure queue is enabled
        while regs.tx_regs.tx_queue[qid].txdctl.read().get_bit(25) != TX_Q_ENABLE {} 

        Ok(vec![tx_descs])
    }  

    /// Enable MSI-X interrupts.
    /// Currently all the msi vectors are for packet reception, one msi vector per receive queue.
    fn enable_msix_interrupts(regs: &mut IntelIxgbeRegisters, rxq: &mut Vec<MutexIrqSafe<RxQueue<AdvancedRxDesc>>>, vector_table: &mut MsixVectorTable, interrupt_handlers: &[HandlerFunc]) -> Result<[u8; NUM_MSI_VEC_ENABLED as usize], &'static str> {
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
            let core_id = rxq[i].lock().cpu_id as u32;
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
        self.regs.eicr.read()
    }

    /// Returns all the receive buffers in one packet,
    /// called for individual queues
    pub fn handle_receive<T: RxDescriptor>(&self, mut rxq: &mut RxQueue<T>) -> Result<(), &'static str> {
        Self::collect_from_queue::<T>(&mut rxq, IXGBE_NUM_RX_DESC as u16, &RX_BUFFER_POOL, IXGBE_RX_BUFFER_SIZE_IN_BYTES)
    }   
    
    /// Transmits a packet on the given tx queue
    fn handle_transmit<T:TxDescriptor>(&self, mut txq: &mut TxQueue<T>, transmit_buffer: TransmitBuffer) -> Result<(), &'static str> {
        Self::send_on_queue::<T>(&mut txq, IXGBE_NUM_TX_DESC as u16, transmit_buffer);
        Ok(())
    }

    /// Collects all the packets for a given queue on a rx interrupt
    fn handle_rx_interrupt(&self, qid: u8) {
        let mut rxq = self.rx_queues[qid as usize].lock();
        if let Err(e) = self.handle_receive(&mut rxq) {
            error!("handle_rx_interrupt(): error handling interrupt: {:?}", e);
        }    
    }

}



/// A helper function to poll the nic receive queues
pub fn rx_poll_mq(_: Option<u64>) -> Result<(), &'static str> {
    let nic_ref = get_ixgbe_nic().ok_or("ixgbe nic not initialized")?.read();
    loop {
        for qid in 0..IXGBE_NUM_RX_QUEUES {
            let mut rxq = nic_ref.rx_queues[qid as usize].lock();
            nic_ref.handle_receive(&mut rxq)?;
        }        
    }
}

/// A generic interrupt handler that can be used for packet reception interrupts for any queue.
/// It returns the interrupt number for the rx queue 'qid'.
fn rx_interrupt_handler(qid: u8) -> Option<u8> {
    let interrupt_num = 
        if let Some(ref ixgbe_nic_ref) = IXGBE_NIC.try() {
            let ixgbe_nic = ixgbe_nic_ref.read();
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










