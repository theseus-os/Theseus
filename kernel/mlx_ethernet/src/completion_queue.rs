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
    pub fn init(&mut self, uar_page: u32, log_cq_size: u8, c_eqn: u8, db_addr: PhysicalAddress) {
        *self = CompletionQueueContext::default();
        self.status.write(U32::new(1 << 20 | 1 << 17)); // collapse all CQE to first | overrun ignore 

        let uar = uar_page & 0xFF_FFFF;
        let size = ((log_cq_size & 0x1F) as u32) << 24;
        self.uar_log_cq_size.write(U32::new(uar | size));

        self.c_eqn.write(U32::new(c_eqn as u32));

        self.dbr_addr_h.write(U32::new((db_addr.value() >> 32) as u32));
        self.dbr_addr_l.write(U32::new(db_addr.value() as u32));
    }
}

#[derive(FromBytes, Default)]
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
    _padding1:              u32,
    mini_cqe_num:           Volatile<U32<BigEndian>>,
    timestamp_h:            Volatile<U32<BigEndian>>,
    timestamp_l:            Volatile<U32<BigEndian>>,
    flow_tag:               Volatile<U32<BigEndian>>,
    owner:                  Volatile<U32<BigEndian>>,
}

const_assert_eq!(core::mem::size_of::<CompletionQueueEntry>(), 64);

#[repr(u8)]
enum CommandQueueEntryOpcode {
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
        let invalid_cqe = (CommandQueueEntryOpcode::InvalidCQE as u32) << 4;
        let hw_ownership = 0x1;
        self.owner.write(U32::new(invalid_cqe | hw_ownership));
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
    entries: Vec<BoxRefMut<MappedPages, [CompletionQueueEntry]>>,
    doorbell: BoxRefMut<MappedPages, CompletionQueueDoorbellRecord>
}

impl CompletionQueue {
    pub fn create(entries_mp: Vec<MappedPages>, doorbell_mp: MappedPages) -> Result<CompletionQueue, &'static str> {
        let mut entries = Vec::with_capacity(entries_mp.len());
        let num_entries_in_page = PAGE_SIZE / core::mem::size_of::<CompletionQueueEntry>();
        for page in entries_mp {
            entries.push(BoxRefMut::new(Box::new(page)).try_map_mut(|mp| mp.as_slice_mut::<CompletionQueueEntry>(0, num_entries_in_page))?);
        }

        let doorbell = BoxRefMut::new(Box::new(doorbell_mp)).try_map_mut(|mp| mp.as_type_mut::<CompletionQueueDoorbellRecord>(0))?;
        Ok( CompletionQueue{entries, doorbell} )
    }

    pub fn init(&mut self) {
        for queue_page in self.entries.iter_mut() {
            for entry in queue_page.iter_mut() {
                entry.init()
        
            }
        }
        self.doorbell.update_ci.write(U32::new(0));
        self.doorbell.arm_ci.write(U32::new(0));
    }

    pub fn hw_owned(&self) -> bool {
        self.entries[0][0].owner.read().get() & 0x1 == 0x1
    }

    pub fn check_packet_transmission(&mut self) {
        debug!("CQ owner: {:#X}", self.entries[0][0].owner.read().get());
        debug!("CQ flow_tag: {:#X}", self.entries[0][0].flow_tag.read().get());
    }
}