use zerocopy::*;
use volatile::Volatile;
use byteorder::BigEndian;
use alloc::{
    vec::Vec,
    boxed::Box
};
use memory::{PhysicalAddress, MappedPages, create_contiguous_mapping};
use kernel_config::memory::PAGE_SIZE;
use owning_ref:: BoxRefMut;


#[derive(FromBytes, Default)]
#[repr(C)]
pub(crate) struct CompletionQueueContext {
    status:                 Volatile<U32<BigEndian>>,
    _padding1:              u32,
    page_offset:            Volatile<U32<BigEndian>>,
    uar_log_cq_size:        Volatile<U32<BigEndian>>,
    cq_max_count_period:    Volatile<U32<BigEndian>>,
    c_eqn:                  Volatile<U32<BigEndian>>,
    log_page_size:          Volatile<U32<BigEndian>>,
    _padding2:              u32,
    last_notified_index:    Volatile<U32<BigEndian>>,
    last_solicit_index:     Volatile<U32<BigEndian>>,
    consumer_counter:       Volatile<U32<BigEndian>>,
    producer_counter:       Volatile<U32<BigEndian>>,
    _padding3:              u64,
    dbr_addr_h:             Volatile<U32<BigEndian>>,
    dbr_addr_l:             Volatile<U32<BigEndian>>,
}

const_assert_eq!(core::mem::size_of::<CompletionQueueContext>(), 64);

impl CompletionQueueContext {
    pub fn init(&mut self, uar_page: u32, log_cq_size: u8, c_eqn: u8, db_addr: PhysicalAddress, collapsed: bool) {
        *self = CompletionQueueContext::default();
        let status = if collapsed {
            1 << 20 | 1 << 17 // collapse all CQE to first | overrun ignore 
        } else {
            1 << 17
        };
        self.status.write(U32::new(status)); 

        let uar = uar_page & 0xFF_FFFF;
        let size = ((log_cq_size & 0x1F) as u32) << 24;
        self.uar_log_cq_size.write(U32::new(uar | size));

        self.c_eqn.write(U32::new(c_eqn as u32));

        let x = libm::ceil((2_usize.pow(log_cq_size as u32) * 64) as f64/ PAGE_SIZE as f64);
        let log_page_size = libm::log2(x) as u32;
        self.log_page_size.write(U32::new(log_page_size << 24));
        
        self.dbr_addr_h.write(U32::new((db_addr.value() >> 32) as u32));
        self.dbr_addr_l.write(U32::new(db_addr.value() as u32));

        trace!("cq context: size: {} {} pg size: {}", x, log_cq_size, log_page_size);
    }
}

#[derive(FromBytes, Debug)]
#[repr(C)]
struct CompletionQueueEntry {
    eth_wqe_id:             Volatile<U32<BigEndian>>,
    lro_tcp_win:            Volatile<U32<BigEndian>>,
    lro_ack_seq_num:        Volatile<U32<BigEndian>>,
    rx_hash_result:         Volatile<U32<BigEndian>>,
    ml_path:                Volatile<U32<BigEndian>>,
    slid_smac:              Volatile<U32<BigEndian>>,
    rqpn:                   Volatile<U32<BigEndian>>,
    vid:                    Volatile<U32<BigEndian>>,
    srqn_user_index:        Volatile<U32<BigEndian>>,
    flow_table_metadata:    Volatile<U32<BigEndian>>,
    _padding1:              Volatile<U32<BigEndian>>,
    mini_cqe_num:           Volatile<U32<BigEndian>>,
    timestamp_h:            Volatile<U32<BigEndian>>,
    timestamp_l:            Volatile<U32<BigEndian>>,
    flow_tag:               Volatile<U32<BigEndian>>,
    owner:                  Volatile<U32<BigEndian>>,
}

const_assert_eq!(core::mem::size_of::<CompletionQueueEntry>(), 64);

#[repr(u8)]
enum CompletionQueueEntryOpcode {
    Requester = 0x0,
    ResponderRDMAWriteWithImmediate = 0x1,
    ResponderSend = 0x2,
    ResponderSendWithImmediate = 0x3,
    ResponderSendWithInvalidate = 0x4,
    ResizeCq = 0x5,
    SignatureError = 0xC, // PRM ERROR: says its 0x12 but thats not possible with 4 bits
    RequesterError = 0xD,
    ResponderError = 0xE,
    InvalidCQE = 0xF
}

impl CompletionQueueEntry {
    pub fn init(&mut self) {
        *self = CompletionQueueEntry::default();
        // let invalid_cqe = (CompletionQueueEntryOpcode::InvalidCQE as u32) << 4;
        // let hw_ownership = 0x1;
        // self.owner.write(U32::new(invalid_cqe | hw_ownership));
    }

    pub fn dump(&self, i: usize) {
        debug!("CQE {}", i);
        unsafe {
            let ptr = self as *const CompletionQueueEntry as *const u32;
            debug!("{:#010x} {:#010x} {:#010x} {:#010x}", (*ptr).to_be(), (*ptr.offset(1)).to_be(), (*ptr.offset(2)).to_be(), (*ptr.offset(3)).to_be());
            debug!("{:#010x} {:#010x} {:#010x} {:#010x}", (*ptr.offset(4)).to_be(), (*ptr.offset(5)).to_be(), (*ptr.offset(6)).to_be(), (*ptr.offset(7)).to_be());
            debug!("{:#010x} {:#010x} {:#010x} {:#010x}", (*ptr.offset(8)).to_be(), (*ptr.offset(9)).to_be(), (*ptr.offset(10)).to_be(), (*ptr.offset(11)).to_be());
            debug!("{:#010x} {:#010x} {:#010x} {:#010x} \n", (*ptr.offset(12)).to_be(), (*ptr.offset(13)).to_be(), (*ptr.offset(14)).to_be(), (*ptr.offset(15)).to_be());
        }
    }
}

impl Default for CompletionQueueEntry {
    fn default() -> Self {
        CompletionQueueEntry{
            eth_wqe_id:             Volatile::new(U32::new(0xFFFF_FFFF)),
            lro_tcp_win:            Volatile::new(U32::new(0xFFFF_FFFF)),
            lro_ack_seq_num:        Volatile::new(U32::new(0xFFFF_FFFF)),
            rx_hash_result:         Volatile::new(U32::new(0xFFFF_FFFF)),
            ml_path:                Volatile::new(U32::new(0xFFFF_FFFF)),
            slid_smac:              Volatile::new(U32::new(0xFFFF_FFFF)),
            rqpn:                   Volatile::new(U32::new(0xFFFF_FFFF)),
            vid:                    Volatile::new(U32::new(0xFFFF_FFFF)),
            srqn_user_index:        Volatile::new(U32::new(0xFFFF_FFFF)),
            flow_table_metadata:    Volatile::new(U32::new(0xFFFF_FFFF)),
            _padding1:              Volatile::new(U32::new(0xFFFF_FFFF)),
            mini_cqe_num:           Volatile::new(U32::new(0xFFFF_FFFF)),
            timestamp_h:            Volatile::new(U32::new(0xFFFF_FFFF)),
            timestamp_l:            Volatile::new(U32::new(0xFFFF_FFFF)),
            flow_tag:               Volatile::new(U32::new(0xFFFF_FFFF)),
            owner:                  Volatile::new(U32::new(0xFFFF_FFFF)),
        }
    }
}

#[derive(FromBytes, Default)]
#[repr(C)]
struct CompletionQueueDoorbellRecord {
    update_ci:          Volatile<U32<BigEndian>>,
    arm_ci:             Volatile<U32<BigEndian>>,
}

const_assert_eq!(core::mem::size_of::<CompletionQueueDoorbellRecord>(), 8);

pub struct CompletionQueue {
    /// Physically-contiguous completion queue entries
    entries: BoxRefMut<MappedPages, [CompletionQueueEntry]>,
    doorbell: BoxRefMut<MappedPages, CompletionQueueDoorbellRecord>
}

impl CompletionQueue {
    pub fn create(entries_mp: MappedPages, num_entries: usize, doorbell_mp: MappedPages) -> Result<CompletionQueue, &'static str> {
        let entries = BoxRefMut::new(Box::new(entries_mp)).try_map_mut(|mp| mp.as_slice_mut::<CompletionQueueEntry>(0, num_entries))?;

        let doorbell = BoxRefMut::new(Box::new(doorbell_mp)).try_map_mut(|mp| mp.as_type_mut::<CompletionQueueDoorbellRecord>(0))?;
        Ok( CompletionQueue{entries, doorbell} )
    }

    pub fn init(&mut self) {
        for entry in self.entries.iter_mut() {
            entry.init()
        }
        self.doorbell.update_ci.write(U32::new(0));
        self.doorbell.arm_ci.write(U32::new(0));
    }

    pub fn hw_owned(&self, entry_num: usize) -> bool {
        // debug!("{:#x?}", self.entries[entry_num]);
        self.entries[entry_num].owner.read().get() & 0x1 == 0x1
    }

    pub fn check_packet_transmission(&mut self, entry_num: usize) {
        debug!("CQ owner: {:#X}", self.entries[entry_num].owner.read().get());
        debug!("CQ flow_tag: {:#X}", self.entries[entry_num].flow_tag.read().get());
    }

    pub fn dump(&self) {
        for (i, entry) in self.entries.iter().enumerate() {
            entry.dump(i)
        }
    }
}