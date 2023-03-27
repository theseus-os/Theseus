//! The Send Queue (SQ) object holds the descriptor ring used to send outgoing messages and packets.
//! The descriptor ring is referred to as a Work Queue Buffer.
//! This module defines the layout of an SQ, the context used to initialize a SQ,
//! the Transport Interface Send object attached to the queue and related functions.
//! 
//! (PRM Section 8.15: Send Queue)

use zerocopy::{U32, FromBytes};
use volatile::Volatile;
use byteorder::BigEndian;
use memory::{PhysicalAddress, MappedPages, BorrowedSliceMappedPages, Mutable, BorrowedMappedPages};
use core::fmt;
use num_enum::TryFromPrimitive;
use core::convert::TryFrom;

#[allow(unused_imports)]
use crate::{ 
    Tisn, Sqn, Lkey, CQN_MASK,
    command_queue::CommandOpcode,
    work_queue::{WorkQueueEntrySend, DoorbellRecord},
    uar::UserAccessRegion
};

/// The Transport Interface Send (TIS) object is responsible for performing all transport
/// related operations of the transmit side. Each SQ is associated with a TIS.
#[derive(FromBytes, Default)]
#[repr(C)]
pub(crate) struct TransportInterfaceSendContext {
    /// A multi-part field:
    /// * `tls_en`: if set, TLS offload is supported, occupies bit 30
    /// * `prio_or_sl`: for Ethernet, Ethernet Priority in bits [19:17], occupies bits [19:16]
    prio_or_sl:         Volatile<U32<BigEndian>>,
    _padding1:          [u8; 32],
    /// transport domain ID
    transport_domain:   Volatile<U32<BigEndian>>,
    _padding2:          u32,
    /// protection domain ID
    pd:                 Volatile<U32<BigEndian>>,
    _padding3:          [u32; 28]
}

const _: () = assert!(core::mem::size_of::<TransportInterfaceSendContext>() == 160);

impl TransportInterfaceSendContext {
    /// Create and initialize a TIS object
    /// 
    /// # Arguments
    /// * `td`: transport domain ID 
    pub fn init(td: u32) -> TransportInterfaceSendContext {
        let mut ctxt = TransportInterfaceSendContext::default();
        ctxt.transport_domain.write(U32::new(td));
        ctxt
    }

    /// Offset that this context is written to in the mailbox buffer
    pub(crate) fn mailbox_offset() -> usize { 0x10 }
}

/// The bitmask for the state in the [`SendQueueContext`]
const STATE_MASK:   u32 = 0xF0_0000;
/// The bit shift for the state in the [`SendQueueContext`]
const STATE_SHIFT:  u32 = 20;

/// The data structure containing SQ initialization parameters.
/// It is passed to the HCA at the time of SQ creation.
#[derive(FromBytes, Default)]
#[repr(C, packed)]
pub(crate) struct SendQueueContext {
    /// A multi-part field:
    /// * `rlky`: when set the reserved LKey can be used on the SQ, occupies bit 31
    /// * `fre`: when set the SQ supports Fast Register WQEs, occupies bit 29
    /// * `flush_in_error_en`: if set, and when SQ transitions into error state, the hardware will flush in error WQEs that were posted, occupies bit 28
    /// * `min_wqe_inline_mode`: sets the inline mode for the SQ, occupies bits [26:24] 
    rlky_state:                         Volatile<U32<BigEndian>>,
    /// an opaque identifier which software sets, which will be reported to the CQ
    user_index:                         Volatile<U32<BigEndian>>,
    /// number of the CQ associated with this SQ
    cqn:                                Volatile<U32<BigEndian>>,
    hairpin_peer_rq:                    Volatile<U32<BigEndian>>,
    hairpin_peer_vhca:                  Volatile<U32<BigEndian>>,
    _padding1:                          u64,
    packet_pacing_rate_limit_index:     Volatile<U32<BigEndian>>,
    /// the number of entries in the list of TISes
    tis_lst_sz:                         Volatile<U32<BigEndian>>,
    _padding2:                          u64,
    /// list of TIS numbers
    tis_num_0:                          Volatile<U32<BigEndian>>,
}

const _: () = assert!(core::mem::size_of::<SendQueueContext>() == 48);

impl fmt::Debug for SendQueueContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("SendQueueContext")
            .field("rlky_state", &self.rlky_state.read().get())
            .field("user_index", &self.user_index.read().get())
            .field("cqn", &self.cqn.read().get())
            .field("hairpin_peer_rq", &self.hairpin_peer_rq.read().get())
            .field("hairpin_peer_vhca",&self.hairpin_peer_vhca.read().get())
            .field("packet_pacing_rate_limit_index",&self.packet_pacing_rate_limit_index.read().get())
            .field("tis_list_sz",&self.tis_lst_sz.read().get())
            .field("tis_num_0",&self.tis_num_0.read().get())
            .finish()
    }
}

/// The possible states the SQ can be in.
#[derive(Debug, TryFromPrimitive)]
#[repr(u8)]
pub enum SendQueueState {
    Reset = 0x0,
    Ready = 0x1,
    Error = 0x3
}

impl SendQueueContext {
    /// Create and initialize the fields of the SQ context.
    /// The SQ context is then passed to the HCA when creating the SQ.
    /// 
    /// # Arguments
    /// * `cqn`: number of CQ associated with this SQ 
    /// * `tisn`: number of the TIS context associated with this SQ
    pub fn init(cqn: u32, tisn: u32) -> SendQueueContext{
        // We are always using 1 TIS per SQ
        const TIS_LST_SZ:               u32 = 1 << 16;
        const TISN_MASK:                u32 = 0xFF_FFFF;
        const ENABLE_RLKEY:             u32 = 1 << 31;
        const FAST_REGISTER_ENABLE:     u32 = 1 << 29;
        const FLUSH_IN_ERROR_ENABLE:    u32 = 1 << 28;
        const ONE_INLINE_HEADER:        u32 = 1 << 24;

        // set all fields to zero
        let mut ctxt = SendQueueContext::default();

        ctxt.rlky_state.write(U32::new(ENABLE_RLKEY | FAST_REGISTER_ENABLE | FLUSH_IN_ERROR_ENABLE | ONE_INLINE_HEADER));
        ctxt.cqn.write(U32::new(cqn & CQN_MASK));
        ctxt.tis_lst_sz.write(U32::new(TIS_LST_SZ));
        ctxt.tis_num_0.write(U32::new(tisn & TISN_MASK));
        ctxt
    }

    /// set state of the SQ in the SQ context to `next_state`
    pub fn set_state(&mut self, next_state: SendQueueState) {
        let state = self.rlky_state.read().get() & !STATE_MASK;
        self.rlky_state.write(U32::new(state | ((next_state as u32) << STATE_SHIFT))); 
    }

    /// Find the state of the SQ from the SQ context 
    pub fn get_state(&self) -> Result<SendQueueState, &'static str> {
        let state = (self.rlky_state.read().get() & STATE_MASK) >> STATE_SHIFT;
        SendQueueState::try_from(state as u8).map_err(|_e| "Invalid value in the SQ state")
    }

    /// Offset that this context is written to in the mailbox buffer
    pub(crate) fn mailbox_offset() -> usize { 0x10 }
}

/// There are two doorbell registers we use to send packets.
/// We alternate between them for each packet.
pub(crate) enum CurrentUARDoorbell {
    Even,
    Odd
}

impl CurrentUARDoorbell {
    fn alternate(&self) -> CurrentUARDoorbell {
        match self {
            Self::Even => Self::Odd,
            Self::Odd => Self::Even,
        }
    }
}

/// A data structure that contains the SQ ring of descriptors 
/// and is used to interact with the SQ once initialized.
pub struct SendQueue {
    /// physically-contiguous SQ descriptors
    entries: BorrowedSliceMappedPages<WorkQueueEntrySend, Mutable>, 
    /// the doorbell for the SQ
    doorbell: BorrowedMappedPages<DoorbellRecord, Mutable>,
    /// the UAR page associated with the SQ
    uar: BorrowedMappedPages<UserAccessRegion, Mutable>,
    /// The number of WQEs that have been completed.
    /// From this we also calculate the next descriptor to use
    wqe_counter: u16,
    /// SQ number that is returned by the [`CommandOpcode::CreateSq`] command
    sqn: Sqn,
    /// number of the TIS context associated with this SQ
    _tisn: Tisn,
    /// the lkey used by the SQ
    lkey: Lkey,
    /// the uar doorbell to be used by the next packet
    uar_db: CurrentUARDoorbell
}

impl SendQueue {
    /// Creates a SQ by mapping the buffer as a slice of [`WorkQueueEntrySend`]s.
    /// Each WQE is set to an initial state.
    /// 
    /// # Arguments
    /// * `entries_mp`: memory that is to be transformed into a slice of WQEs. 
    /// The starting physical address should have been passed to the HCA when creating the SQ.
    /// * `num_entries`: number of entries in the SQ
    /// * `doorbell_mp`: memory that is to be transformed into a doorbell record. 
    /// The starting physical address should have been passed to the HCA when creating the SQ.   
    /// * `uar_mp`: The UAR page that is associate with this SQ. 
    /// * `sqn`: SQ number returned by the HCA
    /// * `tisn`: number of the TIS context associated with this SQ
    /// * `lkey`: the lkey used by the SQ
    pub fn create(
        entries_mp: MappedPages, 
        num_entries: usize, 
        doorbell_mp: MappedPages, 
        uar_mp: MappedPages, 
        sqn: Sqn,
        _tisn: Tisn,
        lkey: Lkey
    ) -> Result<SendQueue, &'static str> {
        // map the descriptor ring and initialize
        let mut entries = entries_mp.into_borrowed_slice_mut::<WorkQueueEntrySend>(0, num_entries)
            .map_err(|(_mp, err)| err)?;
        for entry in entries.iter_mut() {
            entry.init()
        }
        // map the doorbell and initialize
        let mut doorbell = doorbell_mp.into_borrowed_mut(0).map_err(|(_mp, err)| err)?;
        *doorbell = DoorbellRecord::default();
        // map the uar and initialize
        let mut uar = uar_mp.into_borrowed_mut(0).map_err(|(_mp, err)| err)?;
        *uar = UserAccessRegion::default();

        Ok( SendQueue{entries, doorbell, uar, wqe_counter: 0, sqn, _tisn, lkey, uar_db: CurrentUARDoorbell::Even} )
    }

    /// Returns the index into the WQ given the total number of WQEs completed
    fn desc_id(&self) -> usize {
        self.wqe_counter as usize  % self.entries.len()
    }

    /// The steps required to post a WQE after the WQE fields have been initialized.
    /// The doorbell record is updated and the UAR register is written to.
    fn finish_wqe_operation(&mut self) {
        let desc_id = self.desc_id();
        let wqe = &mut self.entries[desc_id];
        // need to wrap around 0xFFFF, this should happen automatically with a u16
        self.wqe_counter += 1; 
        // we're writing the wqe counter, not the next wqe to be used (8.8.2 is confusing about what should actually be posted)
        self.doorbell.send_counter.write(U32::new(self.wqe_counter as u32)); 
        
        let mut doorbell = [U32::new(0);64];
        doorbell[0] = wqe.control.opcode.read(); 
        doorbell[1] = wqe.control.ds.read();
        self.uar.write_wqe_to_doorbell(&self.uar_db, doorbell);
        self.uar_db = self.uar_db.alternate();

        // wqe.dump(desc_id);
    }

    /// Perform all the steps to send a packet: initialize the WQE, update the doorbell record and the uar doorbell register.
    /// Returns the current value of the WQE counter.
    pub fn send(&mut self, packet_address: PhysicalAddress, packet: &[u8]) -> u16 {
        let desc_id = self.desc_id();
        let wqe = &mut self.entries[desc_id];
        wqe.send(self.wqe_counter as u32, self.sqn.0, self.lkey.0, packet_address, packet);
        self.finish_wqe_operation();
        self.wqe_counter
    }

    /// Perform all the steps to complete a NOP: initialize the WQE, update the doorbell record and the uar doorbell register.
    /// Returns the current value of the WQE counter.
    pub fn nop(&mut self) -> u16 {
        let desc_id = self.desc_id();
        let wqe = &mut self.entries[desc_id];
        wqe.nop(self.wqe_counter as u32, self.sqn.0);
        self.finish_wqe_operation();  
        self.wqe_counter
    }

    /// Prints out all entries in the SQ
    pub fn dump(&self) {
        for (i, entry) in self.entries.iter().enumerate() {
            entry.dump(i)
        }
    }
}
