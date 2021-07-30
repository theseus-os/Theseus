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
struct CompletionQueueContext {
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
    dbr_addr:               Volatile<u64>,
}

const_assert_eq!(core::mem::size_of::<CompletionQueueContext>(), 64);

impl CompletionQueueContext {
    pub fn init(&mut self, uar_page: u32, log_cq_size: u8) {
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