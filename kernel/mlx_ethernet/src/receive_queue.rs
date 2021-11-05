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
use core::fmt;

use crate::{
    work_queue::WorkQueueEntrySend,
    uar::UserAccessRegion
};

#[derive(FromBytes, Default)]
#[repr(C, packed)]
pub(crate) struct ReceiveQueueContext {
    rlky_state:                         Volatile<U32<BigEndian>>,
    user_index:                         Volatile<U32<BigEndian>>,
    cqn:                                Volatile<U32<BigEndian>>,
    counter_set_id:                     Volatile<U32<BigEndian>>,
    rmpn:                               Volatile<U32<BigEndian>>,
    hairpin_peer_sq:                    Volatile<U32<BigEndian>>,
    hairpin_peer_vhca:                  Volatile<U32<BigEndian>>,
    _padding1:                          [u8; 20],
}

const_assert_eq!(core::mem::size_of::<ReceiveQueueContext>(), 48);

impl fmt::Debug for ReceiveQueueContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ReceiveQueueContext")
            .field("rlky_state", &self.rlky_state.read().get())
            .field("user_index", &self.user_index.read().get())
            .field("cqn", &self.cqn.read().get())
            .field("counter_set_id", &self.counter_set_id.read().get())
            .field("rmpn", &self.rmpn.read().get())
            .field("hairpin_peer_rq", &self.hairpin_peer_sq.read().get())
            .field("hairpin_peer_vhca",&self.hairpin_peer_vhca.read().get())
            .finish()
    }
}

pub enum ReceiveQueueState {
    Reset = 0x0,
    Ready = 0x1,
    Error = 0x3
}

impl ReceiveQueueContext {
    pub fn init(&mut self, cqn: u32) {
        *self = ReceiveQueueContext::default();
        self.rlky_state.write(U32::new((1 << 31) | (1 << 28))); // enable reserved lkey | VLAN strip disable 
        self.cqn.write(U32::new(cqn & 0xFF_FFFF));
    }

    pub fn set_state(&mut self, next_state: ReceiveQueueState) {
        let state = self.rlky_state.read().get() & !0xF0_0000;
        self.rlky_state.write(U32::new(state | ((next_state as u32) << 20))); 
    }

    pub fn get_state(&self) -> u8 {
        let state = (self.rlky_state.read().get() & 0xF0_0000) >> 20;
        state as u8
    }
}

pub struct ReceiveQueue {
    /// Physically-contiguous queue entries
    entries: MappedPages,
    wqe_index: u32
}

impl ReceiveQueue {
    pub fn create(entries_mp: MappedPages) -> Result<ReceiveQueue, &'static str> {
        Ok( ReceiveQueue{entries: entries_mp, wqe_index: 0} )
    }
}