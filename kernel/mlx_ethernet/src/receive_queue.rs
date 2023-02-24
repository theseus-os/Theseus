//! The Receive Queue (RQ) object holds the descriptor ring used to hold incoming packets.
//! The descriptor ring is referred to as a Work Queue Buffer.
//! This module defines the layout of an RQ and the context used to initialize a RQ.
//! 
//! (PRM Section 8.13: Receive Queue)

use zerocopy::{U32, FromBytes};
use volatile::Volatile;
use byteorder::BigEndian;
use memory::{MappedPages, create_contiguous_mapping, BorrowedSliceMappedPages, Mutable};
use core::fmt;
use num_enum::TryFromPrimitive;
use core::convert::TryFrom;
use alloc::vec::Vec;
use nic_buffers::ReceiveBuffer;
use nic_initialization::NIC_MAPPING_FLAGS;

#[allow(unused_imports)]
use crate::{Rqn, Lkey, CQN_MASK, command_queue::CommandOpcode, work_queue::WorkQueueEntryReceive, completion_queue::CompletionQueue};


/// The Transport Interface Receive (TIR) object is responsible for performing 
/// all transport related operations on the receive side. TIR performs the
/// packet processing and reassembly and is also responsible for demultiplexing
/// packets into different RQs.
#[derive(FromBytes, Default)]
#[repr(C)]
pub(crate) struct TransportInterfaceReceiveContext {
    _padding1:              [u8; 28],
    /// RQ number that packets will directly be delivered to
    inline_rqn:             Volatile<U32<BigEndian>>,
    _padding2:              u32,
    /// transport domain ID
    transport_domain:       Volatile<U32<BigEndian>>,
    _padding3:              [u8; 32],
    _padding4:              [u8; 20],
}

const _: () = assert!(core::mem::size_of::<TransportInterfaceReceiveContext>() == 92);

impl TransportInterfaceReceiveContext {
    /// Initialize the TIR object
    /// 
    /// # Arguments
    /// * `rqn`: RQ number
    /// * `td`: transport domain ID 
    pub fn init(rqn: u32, td: u32) -> TransportInterfaceReceiveContext {
        let mut ctxt = TransportInterfaceReceiveContext::default();
        ctxt.inline_rqn.write(U32::new(rqn));
        ctxt.transport_domain.write(U32::new(td));
        ctxt
    }

    /// Offset that this context is written to in the mailbox buffer
    pub(crate) fn mailbox_offset() -> usize { 0x10 }
}

/// The possible states the RQ can be in.
#[derive(Debug, TryFromPrimitive)]
#[repr(u8)]
pub enum ReceiveQueueState {
    Reset = 0x0,
    Ready = 0x1,
    Error = 0x3
}

/// The bitmask for the state in the [`ReceiveQueueContext`]
const STATE_MASK:   u32 = 0xF0_0000;
/// The bit shift for the state in the [`ReceiveQueueContext`]
const STATE_SHIFT:  u32 = 20;

/// The data structure containing RQ initialization parameters.
/// It is passed to the HCA at the time of RQ creation.
#[derive(FromBytes, Default)]
#[repr(C, packed)]
pub(crate) struct ReceiveQueueContext {
    /// A multi-part field:
    /// * `rlky`: when set the reserved LKey can be used on the RQ, occupies bit 31
    /// * `vlan_strip_disable`: if set, VLAN is not stripped from incoming frames, occupies bit 28
    /// * `state`: RQ state, occupies bits [23:20]
    /// * `flush_in_error_en`: if set, and when RQ transitions into error state, the hardware will flush in error WQEs that were posted, occupies bit 18
    rlky_state:                         Volatile<U32<BigEndian>>,
    /// an opaque identifier which software sets, which is reported to the Completion Queue
    user_index:                         Volatile<U32<BigEndian>>,
    /// number of the CQ associated with this RQ
    cqn:                                Volatile<U32<BigEndian>>,
    /// set of counters in which statistics on this RQ are collected
    counter_set_id:                     Volatile<U32<BigEndian>>,
    /// remote memory pool number (only when enabled)
    rmpn:                               Volatile<U32<BigEndian>>,
    hairpin_peer_sq:                    Volatile<U32<BigEndian>>,
    hairpin_peer_vhca:                  Volatile<U32<BigEndian>>,
    _padding1:                          [u8; 20],
}

const _: () = assert!(core::mem::size_of::<ReceiveQueueContext>() == 48);

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

#[allow(unused)]
impl ReceiveQueueContext {
    /// Create and initialize the fields of the RQ context.
    /// The RQ context is then passed to the HCA when creating the RQ.
    /// 
    /// # Arguments
    /// * `cqn`: number of CQ associated with this RQ 
    pub fn init(cqn: u32) -> ReceiveQueueContext {
        const ENABLE_RLKEY:             u32 = 1 << 31;
        const VLAN_STRIP_DISABLE:       u32 = 1 << 28;
        
        // set all fields to zero
        let mut ctxt = ReceiveQueueContext::default();
        ctxt.rlky_state.write(U32::new(ENABLE_RLKEY | VLAN_STRIP_DISABLE)); 
        ctxt.cqn.write(U32::new(cqn & CQN_MASK));
        ctxt
    }

    /// set state of the RQ in the RQ context to `next_state`
    pub fn set_state(&mut self, next_state: ReceiveQueueState) {
        let state = self.rlky_state.read().get() & !STATE_MASK;
        self.rlky_state.write(U32::new(state | ((next_state as u32) << STATE_SHIFT))); 
    }

    /// Find the state of the RQ from the RQ context 
    pub fn get_state(&self) -> Result<ReceiveQueueState, &'static str> {
        let state = (self.rlky_state.read().get() & STATE_MASK) >> STATE_SHIFT;
        ReceiveQueueState::try_from(state as u8).map_err(|_e| "Invalid value in the RQ state")
    }

    /// Offset that this context is written to in the mailbox buffer
    pub(crate) fn mailbox_offset() -> usize { 0x10 }
}

/// A data structure that contains the RQ ring of descriptors 
/// and is used to interact with the RQ once initialized.
#[allow(dead_code)]
pub struct ReceiveQueue {
    /// physically-contiguous RQ descriptors
    entries: BorrowedSliceMappedPages<WorkQueueEntryReceive, Mutable>, 
    /// the packet buffers in use by the descriptors
    packet_buffers: Vec<ReceiveBuffer>,
    /// The size of a receive buffers in bytes. 
    /// It should be set to the MTU.
    buffer_size_bytes: u32,
    /// Rx buffer pool 
    pool: &'static mpmc::Queue<ReceiveBuffer>,
    /// The number of WQEs that have been completed.
    /// From this we also calculate the next descriptor to use
    wqe_counter: u16,
    /// completion queue index of the next completed packet
    cqe_counter: u16,
    /// CQE ownership value that indicates SW owned
    owner: u8,
    /// RQ number that is returned by the [`CommandOpcode::CreateRq`] command
    rqn: Rqn,
    /// the lkey used by the SQ
    lkey: Lkey,
    /// completion queue associated with this receive queue
    cq: CompletionQueue
}

impl ReceiveQueue {
    /// Creates a RQ by mapping the buffer as a slice of [`WorkQueueEntryReceive`]s.
    /// Each WQE is set to an initial state.
    /// 
    /// # Arguments
    /// * `entries_mp`: memory that is to be transformed into a slice of WQEs. 
    /// The starting physical address should have been passed to the HCA when creating the SQ.
    /// * `num_entries`: number of entries in the RQ
    /// * `mtu`: size of the receive buffers in bytes
    /// * `buffer_pool`: receive buffer pool 
    /// * `rqn`: SQ number returned by the HCA
    /// * `lkey`: the lkey used by the RQ
    pub fn create(
        entries_mp: MappedPages, 
        num_entries: usize,
        mtu: u32,
        pool: &'static mpmc::Queue<ReceiveBuffer>, 
        rqn: Rqn, 
        lkey: Lkey,
        cq: CompletionQueue
    ) -> Result<ReceiveQueue, &'static str> {
        // map the descriptor ring and initialize
        let mut entries = entries_mp.into_borrowed_slice_mut::<WorkQueueEntryReceive>(0, num_entries)
            .map_err(|(_mp, err)| err)?;
        for entry in entries.iter_mut() {
            entry.init()
        }
        Ok(ReceiveQueue {
            entries, 
            packet_buffers: Vec::new(), 
            buffer_size_bytes: mtu,
            pool,
            wqe_counter: 0,
            cqe_counter: 0, 
            owner: 0,
            rqn, 
            lkey,
            cq
        })
    }

    /// Refills the receive queue by updating WQEs with new packet buffers.
    /// Right now we assume that this function is only called once at the point of initialization.
    /// 
    /// TODO:
    /// this function can be shifted to nic_initialization if we remove intel specific actions from those functions
    pub fn refill(&mut self) -> Result<(), &'static str> {
        let buffer_size = self.buffer_size_bytes;
        let mem_pool = self.pool;

        // now that we've created the rx descriptors, we can fill them in with initial values
        let mut rx_bufs_in_use: Vec<ReceiveBuffer> = Vec::with_capacity(self.entries.len());
        for wqe in self.entries.iter_mut()
        {
            // obtain or create a receive buffer for each rx_desc
            let rx_buf = self.pool.pop()
                .ok_or("Couldn't obtain a ReceiveBuffer from the pool")
                .or_else(|_e| {
                    create_contiguous_mapping(buffer_size as usize, NIC_MAPPING_FLAGS)
                        .and_then(|(buf_mapped, buf_paddr)|
                            ReceiveBuffer::new(buf_mapped, buf_paddr, buffer_size as u16, mem_pool)
                        )
                })?;
            let paddr_buf = rx_buf.phys_addr();
            rx_bufs_in_use.push(rx_buf); 

            wqe.update_buffer_info(self.lkey.0, paddr_buf, self.buffer_size_bytes); 
        }
        self.packet_buffers = rx_bufs_in_use;
        Ok(())
    }

}
