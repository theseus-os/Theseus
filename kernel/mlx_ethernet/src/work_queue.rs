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
pub(crate) struct WorkQueue {
    wq_type_signature:                  Volatile<U32<BigEndian>>,
    page_offset_lwm:                    Volatile<U32<BigEndian>>,
    pd:                                 Volatile<U32<BigEndian>>,
    uar_page:                           Volatile<U32<BigEndian>>,
    dbr_addr_h:                         Volatile<U32<BigEndian>>,
    dbr_addr_l:                         Volatile<U32<BigEndian>>,
    hw_counter:                         Volatile<U32<BigEndian>>,
    sw_counter:                         Volatile<U32<BigEndian>>,
    log_wq_stride_pg_sz_sz:             Volatile<U32<BigEndian>>,
    single_stride_log_num_of_bytes:     Volatile<U32<BigEndian>>,
    _padding1:                          [u8; 32],
    _padding2:                          [u8; 32],
    _padding3:                          [u8; 32],
    _padding4:                          [u8; 32],
    _padding5:                          [u8; 24],
}

const_assert_eq!(core::mem::size_of::<WorkQueue>(), 192);

impl WorkQueue {
    pub fn init(&mut self, pd: u32, uar_page: u32, db_addr: PhysicalAddress, log_wq_size: u8) {
        *self = WorkQueue::default();
        self.wq_type_signature.write(U32::new(0x1 << 28)); //cyclic
        self.pd.write(U32::new(pd & 0xFF_FFFF));
        self.uar_page.write(U32::new(uar_page & 0xFF_FFFF));
        self.dbr_addr_h.write(U32::new((db_addr.value() >> 32) as u32));
        self.dbr_addr_l.write(U32::new(db_addr.value() as u32));
        let log_wq_stride = 6; //=64
        self.log_wq_stride_pg_sz_sz.write(U32::new((log_wq_stride << 16) |  (log_wq_size as u32 & 0x1F)));
    }
}

