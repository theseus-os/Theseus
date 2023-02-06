//! Completion Queues (CQ) are circular buffers used by the HCA to post completion reports upon completion of a work request.
//! This module defines the layout of an CQ, the context used to initialize an CQ and related functions.
//! 
//! (PRM Section 8.18: Completion Queues)

use core::{
    convert::TryFrom,
    fmt
};
use bit_field::BitField;
use zerocopy::{U32, FromBytes};
use volatile::Volatile;
use byteorder::BigEndian;
use memory::{PhysicalAddress, MappedPages, BorrowedSliceMappedPages, Mutable, BorrowedMappedPages};
use num_enum::TryFromPrimitive;

#[allow(unused_imports)]
use crate::{
    log_page_size, Cqn, UAR_MASK, LOG_QUEUE_SIZE_MASK, LOG_QUEUE_SIZE_SHIFT, LOG_PAGE_SIZE_SHIFT, HW_OWNERSHIP,
    command_queue::CommandOpcode,
    work_queue::WQEOpcode
};

const CQE_OPCODE_SHIFT:         u32 = 4;

/// The data structure containing CQ initialization parameters.
/// It is passed to the HCA at the time of CQ creation.
/// 
/// (PRM Section 8.18.10: Completion Queue Context)
#[derive(FromBytes, Default)]
#[repr(C)]
pub(crate) struct CompletionQueueContext {
    /// A multi-part field:
    /// * `status`: occupies bits [31:28]
    /// * `cc`: if set all the CQE's are collapsed to the first, occupies bit 20
    /// * `oi`: overrun ignore, allows CQE to be overwritten rather than generating an error, occupies bit 17
    /// * `st`: event delivery state machine, occupies bits [11:8] 
    status:                 Volatile<U32<BigEndian>>,
    _padding1:              u32,
    /// This field must be set to zero
    page_offset:            Volatile<U32<BigEndian>>,
    /// A multi-part field:
    /// * `log_cq_size`: Log (base 2) of the CQ size (in entries), occupies bits [28:24]
    /// * `uar_page`: UAR page this CQ can be accessed through, occupies bits [23:0]
    uar_log_cq_size:        Volatile<U32<BigEndian>>,
    cq_max_count_period:    Volatile<U32<BigEndian>>,
    /// EQ this CQ reports completion events to.
    c_eqn:                  Volatile<U32<BigEndian>>,
    /// Log (base 2) of page size in units of 4KiB
    log_page_size:          Volatile<U32<BigEndian>>,
    _padding2:              u32,
    last_notified_index:    Volatile<U32<BigEndian>>,
    last_solicit_index:     Volatile<U32<BigEndian>>,
    /// Consumer counter. The counter is incremented for each CQE polled from the CQ.
    consumer_counter:       Volatile<U32<BigEndian>>,
    /// Producer Counter. The counter is incremented for each CQE that is written by the HW to the CQ.
    producer_counter:       Volatile<U32<BigEndian>>,
    _padding3:              u64,
    /// Upper 4 bytes of the physical address of the [`CompletionQueueDoorbellRecord`]
    dbr_addr_h:             Volatile<U32<BigEndian>>,
    /// Lower 4 bytes of the physical address of the [`CompletionQueueDoorbellRecord`]
    dbr_addr_l:             Volatile<U32<BigEndian>>,
}

const _: () = assert!(core::mem::size_of::<CompletionQueueContext>() == 64);

impl fmt::Debug for CompletionQueueContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("CompletionQueueContext")
            .field("status", &self.status.read().get())
            .field("page_offset", &self.page_offset.read().get())
            .field("uar_log_cq_size", &self.uar_log_cq_size.read().get())
            .finish()
    }
}

impl CompletionQueueContext {
    /// Create and initialize the fields of the CQ context.
    /// The CQ context is then passed to the HCA when creating the CQ.
    /// 
    /// # Arguments
    /// * `uar_page`: UAR page the CQ can be accessed through. 
    /// * `cq_size`: number of entries in the CQ.
    /// * `c_eqn`: number of the EQ this CQ reports completion events to.
    /// * `db_addr`: physical address of the [`CompletionQueueDoorbellRecord`].
    /// * `collapsed`: set to true if all CQE's are collapsed to the first.
    pub fn init(uar_page: u32, cq_size: u32, c_eqn: u8, db_addr: PhysicalAddress, collapsed: bool) -> CompletionQueueContext {
        const COLLAPSE_CQE:     u32 = 1 << 20;
        const OVERRUN_IGNORE:   u32 = 1 << 17;

        // set all fields to zero
        let mut ctxt = CompletionQueueContext::default();
        let mut status = OVERRUN_IGNORE; 
        if collapsed {
            status |= COLLAPSE_CQE; 
        }
        ctxt.status.write(U32::new(status)); 

        let uar = uar_page & UAR_MASK;
        let size = (libm::log2(cq_size as f64) as u32 & LOG_QUEUE_SIZE_MASK) << LOG_QUEUE_SIZE_SHIFT;
        ctxt.uar_log_cq_size.write(U32::new(uar | size));

        ctxt.c_eqn.write(U32::new(c_eqn as u32));

        let log_page_size = log_page_size(cq_size * core::mem::size_of::<CompletionQueueEntry>() as u32); 
        ctxt.log_page_size.write(U32::new(log_page_size << LOG_PAGE_SIZE_SHIFT));
        
        ctxt.dbr_addr_h.write(U32::new((db_addr.value() >> 32) as u32));
        ctxt.dbr_addr_l.write(U32::new(db_addr.value() as u32));
        ctxt
    }

    /// Offset that this context is written to in the mailbox buffer
    pub(crate) fn mailbox_offset() -> usize { 0 }
}

#[derive(Debug, TryFromPrimitive, PartialEq)]
#[repr(u8)]
pub(crate) enum CQEOpcode {
    Requester = 0x0,
    ResponderRDMAWriteWithImmediate = 0x1,
    ResponderSend = 0x2,
    ResponderSendWithImmediate = 0x3,
    ResponderSendWithInvalidate = 0x4,
    ResizeCq = 0x5,
    SignatureError = 0xC, // PRM ERROR: says its 0x12 but thats not possible with 4 bits
    RequesterError = 0xD,
    ResponderError = 0xE,
    InvalidCQE = 0xF,
    Unknown
}

#[allow(dead_code)]
#[repr(u8)]
enum CQEFormat {
    NoInlineData = 0x0,
    InlineData32 = 0x1,
    InlineData64 = 0x2,
    CompressedCQE = 0x3,
}

/// The layout of an entry in the CQ buffer.
/// 
/// (PRM Section 8.18.1.1: CQE Format)
#[derive(FromBytes, Debug, Default)]
#[repr(C)]
pub struct CompletionQueueEntry {
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
    /// Byte count of data transferred. Can be used to find length of received packets.
    byte_count:             Volatile<U32<BigEndian>>,
    timestamp_h:            Volatile<U32<BigEndian>>,
    timestamp_l:            Volatile<U32<BigEndian>>,
    /// A multi-part field:
    /// * `send_wqe_opcode/rx_drop_counter`: the send WQE opcode or the number of dropped packets
    /// because of no RCV WQE since the last CQE, occupies bits \[31:24\]
    flow_tag:               Volatile<U32<BigEndian>>,
    /// A multi-part field:
    /// * `wqe_counter`: wqe_counter of the WQE completed, occupies bits \[31:16\]
    /// * `signature`: byte-wise XOR of CQE, occupies bits \[15:8\]
    /// * `opcode`: a [`CQEOpcode`] value, occupies bits \[7:4\]
    /// * `cqe_format`: a [`CQEFormat`] value, occupies bits \[3:2\]
    /// * `se`: solicited event. This CQE cause EQE generation for solicited event, occupies bit 1
    /// * `owner`: owner of the entry, occupies bit 0.
    /// The value indicating SW ownership is flipped every time CQ wraps around, starting with 0.
    owner:                  Volatile<U32<BigEndian>>,
}

const _: () = assert!(core::mem::size_of::<CompletionQueueEntry>() == 64);

#[allow(unused)]
impl CompletionQueueEntry {
    pub fn init(&mut self) {
        // Snabb initializes the CQE but setting all the bits. I do not think that is correct.
        // In section 23.9.1: CREATE_CQ it stated that only the opcode and owner bit need to be set
        
        // set all fields to zero
        *self = CompletionQueueEntry::default();
        let invalid_cqe = (CQEOpcode::InvalidCQE as u32) << CQE_OPCODE_SHIFT;
        self.owner.write(U32::new(invalid_cqe | HW_OWNERSHIP));
    }

    /// Return the WQE opcode value of the WQE completed
    pub(crate) fn get_send_wqe_opcode(&self) -> Result<WQEOpcode, &'static str> {
        const WQE_OPCODE_SHIFT: u32 = 24;
        WQEOpcode::try_from((self.flow_tag.read().get() >> WQE_OPCODE_SHIFT) as u8)
            .map_err(|_e| "Invalid WQE opcode in the CQE")
    }

    /// Return the WQE counter value for the WQE completed
    pub(crate) fn get_wqe_counter(&self) -> u16 {
        const WQE_COUNTER_SHIFT: u32 = 16;
        (self.owner.read().get() >> WQE_COUNTER_SHIFT) as u16
    }

    /// Returns true if the ownership bit is set
    pub(crate) fn get_owner(&self) -> bool {
        self.owner.read().get().get_bit(0)
    }

    /// Returns the WQE entry opcode
    pub(crate) fn get_opcode(&self) -> CQEOpcode {
        CQEOpcode::try_from((self.owner.read().get() >> 4 & 0xF) as u8)
            .unwrap_or(CQEOpcode::Unknown)
    }

    /// Returns the length of the received packet
    pub(crate) fn get_pkt_len(&self) -> u32 {
        self.byte_count.read().get()
    }

    /// Prints out the fields of a CQE in the format used by other drivers (e.g. Linux, Snabb)
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

/// A structure containing information of recently-posted CQ commands
#[derive(FromBytes, Default)]
#[repr(C)]
pub struct CompletionQueueDoorbellRecord {
    /// Consumer counter of the last polled CQE.
    /// It points to the next CQE to be polled.
    update_ci:          Volatile<U32<BigEndian>>,
    /// Consumer Counter for arming CQ
    arm_ci:             Volatile<U32<BigEndian>>,
}

const _: () = assert!(core::mem::size_of::<CompletionQueueDoorbellRecord>() == 8);

/// A data structure that contains the CQ buffer 
/// and is used to interact with the CQ once initialized.
#[allow(dead_code)]
pub struct CompletionQueue {
    /// Physically-contiguous completion queue entries
    pub(crate) entries: BorrowedSliceMappedPages<CompletionQueueEntry, Mutable>,
    /// Doorbell record for this CQ
    doorbell: BorrowedMappedPages<CompletionQueueDoorbellRecord, Mutable>,
    /// CQ number that is returned by the [`CommandOpcode::CreateCq`] command
    cqn: Cqn,
}

impl CompletionQueue {
    /// Creates a completion queue by mapping the buffer as a slice of [`CompletionQueueEntry`]s.
    /// Each CQE is set to an initial state.
    /// 
    /// # Arguments
    /// * `entries_mp`: memory that is to be transformed into a slice of CQEs. 
    ///    The starting physical address should have been passed to the HCA when creating the CQ.
    /// * `num_entries`: number of entries in the CQ
    /// * `doorbell_mp`: memory that is to be transformed into a [`CompletionQueueDoorbellRecord`]. 
    ///    The starting physical address should have been passed to the HCA when creating the CQ.
    /// * `cqn`: CQ number returned by the HCA
    pub fn init(
        entries_mp: MappedPages, 
        num_entries: usize, 
        doorbell_mp: MappedPages,
        cqn: Cqn
    ) -> Result<CompletionQueue, &'static str> {
        let mut entries = entries_mp.into_borrowed_slice_mut::<CompletionQueueEntry>(0, num_entries)
            .map_err(|(_mp, err)| err)?;
        let mut doorbell = doorbell_mp.into_borrowed_mut(0)
            .map_err(|(_mp, err)| err)?;
        
        for entry in entries.iter_mut() {
            entry.init()
        }
        *doorbell = CompletionQueueDoorbellRecord::default();

        Ok( CompletionQueue { entries, doorbell, cqn } )
    }

    /// Checks if a packet is transmitted by comparing the `wqe_counter` with the value in the CQE.
    /// If it is, then prints out the WQE opcode and counter.
    pub fn check_packet_transmission(&mut self, entry_num: usize, wqe_counter: u16) {
        let entry = &self.entries[entry_num];
        let counter = entry.get_wqe_counter();
        if wqe_counter == counter {
            trace!("opcode: {:?}, wqe_counter: {}", entry.get_send_wqe_opcode(), counter);
        }
    }

    /// Prints out all entries in the CQ
    pub fn dump(&self) {
        for (i, entry) in self.entries.iter().enumerate() {
            entry.dump(i)
        }
    }
}
