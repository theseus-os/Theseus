//! Event Queues (EQ) are circular buffers used by the HCA to report completion events, errors and other asynchronous events.
//! This module defines the layout of an EQ, the context used to initialize an EQ and related functions.
//! 
//! (PRM Section 8.19: Events and Interrupts)

use zerocopy::{U32, FromBytes};
use volatile::Volatile;
use byteorder::BigEndian;
use memory::{MappedPages, BorrowedSliceMappedPages, Mutable};
#[allow(unused_imports)]
use crate::{
    log_page_size, Cqn, Eqn, UAR_MASK, LOG_QUEUE_SIZE_MASK, LOG_QUEUE_SIZE_SHIFT, LOG_PAGE_SIZE_SHIFT, HW_OWNERSHIP,
    command_queue::CommandOpcode
};


/// The data structure containing EQ initialization parameters.
/// It is passed to the HCA at the time of EQ creation.
/// 
/// (PRM Section 8.19.17: Event Queue Context)
#[derive(FromBytes, Default)]
#[repr(C)]
pub(crate) struct EventQueueContext {      
    /// A multi-part field:
    /// * `status`: occupies bits [31:28]
    /// * `ec`: if set all the EQE's are collapsed to the first, occupies bit 18
    /// * `st`: event delivery state machine, occupies bits [11:8] 
    status:             Volatile<U32<BigEndian>>, 
    _padding1:          u32,
    /// This field must be set to zero
    page_offset:        Volatile<U32<BigEndian>>,
    /// A multi-part field:
    /// * `log_eq_size`: Log (base 2) of the EQ size (in entries), occupies bits [28:24]
    /// * `uar_page`: UAR page this EQ can be accessed through, occupies bits [23:0]
    uar_log_eq_size:    Volatile<U32<BigEndian>>,
    _padding2:          u32,
    /// MSI-X table entry index to be used to signal interrupts on this EQ
    intr:               Volatile<U32<BigEndian>>,
    /// Log (base 2) of page size in units of 4KiB
    log_pg_size:        Volatile<U32<BigEndian>>,
    _padding3:          [u8;12],
    /// Consumer counter. The counter is incremented for each EQE polled from the EQ.
    consumer_counter:   Volatile<U32<BigEndian>>,
    /// Producer Counter. The counter is incremented for each EQE that is written by the HW to the EQ.
    producer_counter:   Volatile<U32<BigEndian>>,
    _padding4:          [u8;16],
}

const _: () = assert!(core::mem::size_of::<EventQueueContext>() == 64);

impl EventQueueContext {
    /// Create and initialize the fields of the EQ context.
    /// The EQ context is then passed to the HCA when creating the EQ.
    /// 
    /// # Arguments
    /// * `uar_page`: UAR page the EQ can be accessed through. 
    /// * `eq_size`: number of entries in the EQ.
    pub(crate) fn init(uar_page: u32, eq_size: u32) -> EventQueueContext {
        // set all entries to zero
        let mut ctxt = EventQueueContext::default();

        // initialize all other required fields
        let uar = uar_page & UAR_MASK;
        let size = (libm::log2(eq_size as f64) as u32 & LOG_QUEUE_SIZE_MASK) << LOG_QUEUE_SIZE_SHIFT;
        ctxt.uar_log_eq_size.write(U32::new(uar | size));
        
        let log_eq_page_size = log_page_size(eq_size * core::mem::size_of::<EventQueueEntry>() as u32);
        ctxt.log_pg_size.write(U32::new(log_eq_page_size << LOG_PAGE_SIZE_SHIFT));
        ctxt
    }

    /// Offset that this context is written to in the mailbox buffer
    pub(crate) fn mailbox_offset() -> usize { 0 }
}

/// The layout of an entry in the EQ buffer.
/// 
/// (PRM Section 8.19.2.2: EQE Format)
#[derive(FromBytes, Default, Debug)]
#[repr(C)]
pub struct EventQueueEntry {
    event_type: Volatile<U32<BigEndian>>,
    _padding1: [u8; 28],
    /// delivers auxiliary data to handle the event
    event_data: Volatile<[u8; 28]>,
    /// A multi-part field:
    /// * `signature`: byte-wise XOR of EQE, occupies bits \[15:8\]
    /// * `owner`: owner of the entry, occupies bit 0
    signature_owner: Volatile<U32<BigEndian>>
}

const _: () = assert!(core::mem::size_of::<EventQueueEntry>() == 64);

impl EventQueueEntry {
    pub fn init(&mut self) {
        // set all fields to zero
        *self = EventQueueEntry::default();
        // all EQEs must initially be set to HW ownership
        self.signature_owner.write(U32::new(HW_OWNERSHIP)); 
        // Snabb, I believe, has this wrong. 
        // They are setting the ownership bit within the padding field
    }

    /// Prints out the fields of an EQE in the format used by other drivers (e.g. Linux, Snabb)
    pub fn dump(&self, i: usize) {
        debug!("EQE {}", i);
        unsafe {
            let ptr = self as *const EventQueueEntry as *const u32;
            debug!("{:#010x} {:#010x} {:#010x} {:#010x}", (*ptr).to_be(), (*ptr.offset(1)).to_be(), (*ptr.offset(2)).to_be(), (*ptr.offset(3)).to_be());
            debug!("{:#010x} {:#010x} {:#010x} {:#010x}", (*ptr.offset(4)).to_be(), (*ptr.offset(5)).to_be(), (*ptr.offset(6)).to_be(), (*ptr.offset(7)).to_be());
            debug!("{:#010x} {:#010x} {:#010x} {:#010x}", (*ptr.offset(8)).to_be(), (*ptr.offset(9)).to_be(), (*ptr.offset(10)).to_be(), (*ptr.offset(11)).to_be());
            debug!("{:#010x} {:#010x} {:#010x} {:#010x} \n", (*ptr.offset(12)).to_be(), (*ptr.offset(13)).to_be(), (*ptr.offset(14)).to_be(), (*ptr.offset(15)).to_be());
        }
    }

 }

/// A data structure that contains the EQ buffer 
/// and is used to interact with the EQ once initialized.
#[allow(dead_code)]
pub struct EventQueue {
    /// Physically-contiguous event queue entries
    entries: BorrowedSliceMappedPages<EventQueueEntry, Mutable>,
    /// EQ number that is returned by the [`CommandOpcode::CreateEq`] command
    eqn: Eqn
}

impl EventQueue {
    /// Creates an event queue by mapping the buffer as a slice of [`EventQueueEntry`]s.
    /// Each EQE is set to an initial state.
    /// 
    /// # Arguments
    /// * `mp`: memory that is to be transformed into a slice of EQEs. 
    /// The starting physical address should have been passed to the HCA when creating the EQ.
    /// * `num_entries`: number of entries in the EQ
    /// * `eqn`: EQ number returned by the HCA
    pub fn init(mp: MappedPages, num_entries: usize, eqn: Eqn) -> Result<EventQueue, &'static str> {
        let mut entries = mp.into_borrowed_slice_mut::<EventQueueEntry>(0, num_entries)
            .map_err(|(_mp, err)| err)?;
        for eqe in entries.iter_mut() {
            eqe.init()
        }
        Ok( EventQueue { entries, eqn } )
    }

    /// Prints out all entries in the EQ
    pub fn dump(&self) {
        for (i, eqe) in self.entries.iter().enumerate() {
            eqe.dump(i)
        }
    }
}
