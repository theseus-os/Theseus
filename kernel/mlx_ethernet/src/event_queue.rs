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
pub(crate) struct EventQueueContext {
    status:             Volatile<U32<BigEndian>>,
    _padding1:          u32,
    page_offset:        Volatile<U32<BigEndian>>,
    uar_log_eq_size:    Volatile<U32<BigEndian>>,
    _padding2:          u32,
    intr:               Volatile<U32<BigEndian>>,
    log_pg_size:        Volatile<U32<BigEndian>>,
    _padding3:          [u8;12],
    consumer_counter:   Volatile<U32<BigEndian>>,
    producer_counter:   Volatile<U32<BigEndian>>,
    _padding4:          [u8;16],
}

const_assert_eq!(core::mem::size_of::<EventQueueContext>(), 64);

impl EventQueueContext {
    pub fn init(&mut self, uar_page: u32, log_eq_size: u8) {
        *self = EventQueueContext::default();
        let uar = uar_page & 0xFF_FFFF;
        let size = ((log_eq_size & 0x1F) as u32) << 24;
        self.uar_log_eq_size.write(U32::new(uar | size));
        self.log_pg_size.write(U32::new(0));
    }
}

#[derive(FromBytes, Default)]
#[repr(C)]
pub struct EventQueueEntry {
    event_type: Volatile<U32<BigEndian>>,
    _padding1: [u8; 28],
    event_data: Volatile<[u8; 28]>,
    signature_owner: Volatile<U32<BigEndian>>
}

const_assert_eq!(core::mem::size_of::<EventQueueContext>(), 64);

impl EventQueueEntry {
    pub fn init(&mut self) {
        *self = EventQueueEntry::default();
        let hw_ownership = 0x1;
        self.signature_owner.write(U32::new(hw_ownership));
    }
}

pub struct EventQueue {
    /// Physically-contiguous event queue entries
    entries: Vec<BoxRefMut<MappedPages, [EventQueueEntry]>>,
}

impl EventQueue {
    pub fn create(mp: Vec<MappedPages>) -> Result<EventQueue, &'static str> {
        let mut entries = Vec::with_capacity(mp.len());
        let num_entries_in_page = PAGE_SIZE / core::mem::size_of::<EventQueueEntry>();
        for page in mp {
            entries.push(BoxRefMut::new(Box::new(page)).try_map_mut(|mp| mp.as_slice_mut::<EventQueueEntry>(0, num_entries_in_page))?);
        }
        Ok( EventQueue{entries} )
    }

    pub fn init(&mut self) {
        for queue_page in self.entries.iter_mut() {
            for entry in queue_page.iter_mut() {
                entry.init()
        
            }
        }
    }
}