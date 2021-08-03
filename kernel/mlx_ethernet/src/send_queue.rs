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
pub(crate) struct TransportInterfaceSendContext {
    prio_or_sl:         Volatile<U32<BigEndian>>,
    _padding1:          [u8; 32],
    transport_domain:   Volatile<U32<BigEndian>>,
    _padding2:          u32,
    pd:                 Volatile<U32<BigEndian>>,
    _padding3:          [u8; 32],
    _padding4:          [u8; 32],
    _padding5:          [u8; 32],
    _padding6:          [u8; 16],
}

const_assert_eq!(core::mem::size_of::<TransportInterfaceSendContext>(), 160);

impl TransportInterfaceSendContext {
    pub fn init(&mut self, td: u32) {
        *self = TransportInterfaceSendContext::default();
        self.transport_domain.write(U32::new(td));
    }
}


#[derive(FromBytes, Default)]
#[repr(C, packed)]
pub(crate) struct SendQueueContext {
    rlky:                               Volatile<U32<BigEndian>>,
    user_index:                         Volatile<U32<BigEndian>>,
    cqn:                                Volatile<U32<BigEndian>>,
    hairpin_peer_rq:                    Volatile<U32<BigEndian>>,
    hairpin_peer_vhca:                  Volatile<U32<BigEndian>>,
    _padding1:                          u64,
    packet_pacing_rate_limit_index:     Volatile<U32<BigEndian>>,
    tis_lst_sz:                         Volatile<U32<BigEndian>>,
    _padding2:                          u64,
    tis_num_0:                          Volatile<U32<BigEndian>>,
}

const_assert_eq!(core::mem::size_of::<SendQueueContext>(), 48);

impl SendQueueContext {
    pub fn init(&mut self, cqn: u32, tisn: u32) {
        *self = SendQueueContext::default();
        self.rlky.write(U32::new((1 << 31) | (1 << 29) | (1 << 28) | (1 << 24))); // enable reserved lkey | fast register enable |  flush in error WQEs | min_wqe_inline_mode
        self.cqn.write(U32::new(cqn & 0xFF_FFFF));
        self.tis_lst_sz.write(U32::new(1 << 16));
        self.tis_num_0.write(U32::new(tisn & 0xFF_FFFF));
    }
}

// #[derive(FromBytes, Default)]
// #[repr(C)]
// struct CompletionQueueEntry {
//     eth_wqe_id:             Volatile<U32<BigEndian>>,
//     lro_tcp_win:            Volatile<U32<BigEndian>>,
//     lro_ack_seq_num:        Volatile<U32<BigEndian>>,
//     rx_hash_result:         Volatile<U32<BigEndian>>,
//     ml_path:                Volatile<U32<BigEndian>>,
//     slid_smac:              Volatile<U32<BigEndian>>,
//     rqpn:                   Volatile<U32<BigEndian>>,
//     vid:                    Volatile<U32<BigEndian>>,
//     srqn_user_index:        Volatile<U32<BigEndian>>,
//     flow_table_metadata:    Volatile<U32<BigEndian>>,
//     _padding1:              u32,
//     mini_cqe_num:           Volatile<U32<BigEndian>>,
//     timestamp_h:            Volatile<U32<BigEndian>>,
//     timestamp_l:            Volatile<U32<BigEndian>>,
//     flow_tag:               Volatile<U32<BigEndian>>,
//     owner:                  Volatile<U32<BigEndian>>,
// }

// const_assert_eq!(core::mem::size_of::<CompletionQueueEntry>(), 64);

// #[repr(u8)]
// enum CommandQueueEntryOpcode {
//     Requester = 0x0,
//     ResponderRDMAWriteWithImmediate = 0x1,
//     ResponderSend = 0x2,
//     ResponderSendWithImmediate = 0x3,
//     ResponderSendWithInvalidate = 0x4,
//     ResizeCq = 0x5,
//     SignatureError = 0xC, // PRM ERROR: says its 0x12 but thats not possible with 4 bits
//     RequesterError = 0xD,
//     ResponderError = 0xE,
//     InvalidCQE = 0xF
// }

// impl CompletionQueueEntry {
//     pub fn init(&mut self) {
//         *self = CompletionQueueEntry::default();
//         let invalid_cqe = (CommandQueueEntryOpcode::InvalidCQE as u32) << 4;
//         let hw_ownership = 0x1;
//         self.owner.write(U32::new(invalid_cqe | hw_ownership));
//     }
// }

#[derive(FromBytes, Default)]
#[repr(C)]
struct DoorbellRecord {
    rcv_counter:    Volatile<U32<BigEndian>>,
    send_counter:   Volatile<U32<BigEndian>>,
}

const_assert_eq!(core::mem::size_of::<DoorbellRecord>(), 8);

pub struct SendQueue {
    /// Physically-contiguous queue entries
    entries: Vec<MappedPages>, //Vec<BoxRefMut<MappedPages, [CompletionQueueEntry]>>,
    doorbell: BoxRefMut<MappedPages, DoorbellRecord>
}

impl SendQueue {
    pub fn create(entries_mp: Vec<MappedPages>, doorbell_mp: MappedPages) -> Result<SendQueue, &'static str> {
        let doorbell = BoxRefMut::new(Box::new(doorbell_mp)).try_map_mut(|mp| mp.as_type_mut::<DoorbellRecord>(0))?;
        Ok( SendQueue{entries: entries_mp, doorbell} )
    }

    // pub fn init(&mut self) {
    //     for queue_page in self.entries.iter_mut() {
    //         for entry in queue_page.iter_mut() {
    //             entry.init()
        
    //         }
    //     }
    //     self.doorbell.update_ci.write(U32::new(0));
    //     self.doorbell.arm_ci.write(U32::new(0));
    // }
}