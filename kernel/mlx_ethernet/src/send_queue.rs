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
    work_queue::WorkQueueEntry,
    uar::UserAccessRegion
};


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

    pub fn get_state(&self) -> u8 {
        let state = (self.rlky_state.read().get() & 0xF0_0000) >> 20;
        state as u8
    }
}

#[derive(FromBytes, Default)]
#[repr(C)]
struct DoorbellRecord {
    /// wqe_counter
    rcv_counter:    Volatile<U32<BigEndian>>,
    /// sq_wqebb_counter
    send_counter:   Volatile<U32<BigEndian>>,
}

const_assert_eq!(core::mem::size_of::<DoorbellRecord>(), 8);

pub struct SendQueue {
    /// Physically-contiguous queue entries
    entries: BoxRefMut<MappedPages, [WorkQueueEntry]>, 
    doorbell: BoxRefMut<MappedPages, DoorbellRecord>,
    uar: BoxRefMut<MappedPages, UserAccessRegion>,
    wqe_index: u32,
    sqn: u32
}

impl SendQueue {
    pub fn create(entries_mp: MappedPages, doorbell_mp: MappedPages, uar_mp: MappedPages, num_entries: usize, sqn: u32) -> Result<SendQueue, &'static str> {
        let mut doorbell = BoxRefMut::new(Box::new(doorbell_mp)).try_map_mut(|mp| mp.as_type_mut::<DoorbellRecord>(0))?;
        doorbell.send_counter.write(U32::new(0));
        doorbell.rcv_counter.write(U32::new(0));

        let mut uar = BoxRefMut::new(Box::new(uar_mp)).try_map_mut(|mp| mp.as_type_mut::<UserAccessRegion>(0))?;
        uar.db_blueflame_buffer0_even.write([U32::new(0); 64]);
        uar.db_blueflame_buffer0_odd.write([U32::new(0); 64]);

        let mut entries = BoxRefMut::new(Box::new(entries_mp)).try_map_mut(|mp| mp.as_slice_mut::<WorkQueueEntry>(0, num_entries))?;
        for entry in entries.iter_mut() {
            entry.init()
        }

        Ok( SendQueue{entries: entries, doorbell, uar, wqe_index: 0, sqn} )
    }

    pub fn send(&mut self, sqn: u32, tisn: u32, lkey: u32, packet_address: PhysicalAddress) -> Result<(), &'static str> {
        let mut wqe = &mut self.entries[0];
        wqe.init_send(self.wqe_index, sqn, tisn, lkey, packet_address);
        self.wqe_index += 1; // need to wrap around 0xFFFF
        self.doorbell.send_counter.write(U32::new(self.wqe_index));
        let mut doorbell = [U32::new(0);64];
        doorbell[0] = wqe.control.opcode.read(); 
        doorbell[1] = wqe.control.ds.read();
        self.uar.db_blueflame_buffer0_even.write(doorbell);

        Ok(())
    }

    pub fn nop(&mut self, sqn: u32, tisn: u32, lkey: u32) -> Result<(), &'static str> {
        let mut wqe = &mut self.entries[0];
        wqe.nop(self.wqe_index, sqn, tisn, lkey);
        wqe.dump(0);

        self.wqe_index += 1; // need to wrap around 0xFFFF

        wqe.dump(0);
        
        let mut doorbell = [U32::new(0);64];
        doorbell[0] = wqe.control.opcode.read(); 
        doorbell[1] = wqe.control.ds.read();
        self.doorbell.send_counter.write(U32::new(self.wqe_index));

        wqe.dump(0);

        self.uar.db_blueflame_buffer0_even.write(doorbell);

        debug!("{:#X}", self.uar.db_blueflame_buffer0_even.read()[0].get());
        debug!("{:#X}", self.uar.db_blueflame_buffer0_even.read()[1].get());

        wqe.dump(0);

        Ok(())
    }

    pub fn dump(&self) {
        for (i, entry) in self.entries.iter().enumerate() {
            entry.dump(i)
        }
    }
}