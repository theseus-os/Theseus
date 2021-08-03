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

use crate::work_queue::WorkQueueEntry;


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
    rlky_state:                         Volatile<U32<BigEndian>>,
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

pub enum SendQueueState {
    Reset = 0x0,
    Ready = 0x1,
    Error = 0x3
}

impl SendQueueContext {
    pub fn init(&mut self, cqn: u32, tisn: u32) {
        *self = SendQueueContext::default();
        self.rlky_state.write(U32::new((1 << 31) | (1 << 29) | (1 << 28) | (1 << 24))); // enable reserved lkey | fast register enable |  flush in error WQEs | min_wqe_inline_mode
        self.cqn.write(U32::new(cqn & 0xFF_FFFF));
        self.tis_lst_sz.write(U32::new(1 << 16));
        self.tis_num_0.write(U32::new(tisn & 0xFF_FFFF));
    }

    pub fn set_state(&mut self, next_state: SendQueueState) {
        let state = self.rlky_state.read().get() & !0xF0_0000;
        self.rlky_state.write(U32::new(state | ((next_state as u32) << 20))); 
    }
}

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
    doorbell: BoxRefMut<MappedPages, DoorbellRecord>,
    wqe_index: u32
}

impl SendQueue {
    pub fn create(entries_mp: Vec<MappedPages>, doorbell_mp: MappedPages) -> Result<SendQueue, &'static str> {
        let mut doorbell = BoxRefMut::new(Box::new(doorbell_mp)).try_map_mut(|mp| mp.as_type_mut::<DoorbellRecord>(0))?;
        doorbell.send_counter.write(U32::new(0));
        doorbell.rcv_counter.write(U32::new(0));

        Ok( SendQueue{entries: entries_mp, doorbell, wqe_index: 0} )
    }

    pub fn send(&mut self, sqn: u32, tisn: u32, lkey: u32, packet_address: PhysicalAddress) -> Result<(), &'static str> {
        let mut wqe = self.entries[0].as_type_mut::<WorkQueueEntry>(0).map_err(|_e| "Could not map to WQE")?;
        wqe.init(self.wqe_index, sqn, tisn, lkey, packet_address);
        self.wqe_index += 1; // need to wrap around 0xFFFF
        self.doorbell.send_counter.write(U32::new(self.wqe_index));

        Ok(())
    }
}