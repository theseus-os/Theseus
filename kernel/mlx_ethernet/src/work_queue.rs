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
    pub fn init_sq(&mut self, pd: u32, uar_page: u32, db_addr: PhysicalAddress, log_wq_size: u8) {
        *self = WorkQueue::default();
        self.wq_type_signature.write(U32::new(0x1 << 28)); //cyclic
        self.pd.write(U32::new(pd & 0xFF_FFFF));
        self.uar_page.write(U32::new(uar_page & 0xFF_FFFF));
        self.dbr_addr_h.write(U32::new((db_addr.value() >> 32) as u32));
        self.dbr_addr_l.write(U32::new(db_addr.value() as u32));
        let log_wq_stride = libm::log2(64.0) as u32; //=64
        let log_wq_page_size = libm::log2(libm::ceil((2_usize.pow(log_wq_size as u32) * 64) as f64 / PAGE_SIZE as f64)) as u32;
        self.log_wq_stride_pg_sz_sz.write(U32::new((log_wq_stride << 16) | (log_wq_page_size << 8) | (log_wq_size as u32 & 0x1F)));
    }

    pub fn init_rq(&mut self, pd: u32, db_addr: PhysicalAddress, log_wq_size: u8) {
        *self = WorkQueue::default();
        self.wq_type_signature.write(U32::new(0x1 << 28)); //cyclic
        self.pd.write(U32::new(pd & 0xFF_FFFF));
        self.dbr_addr_h.write(U32::new((db_addr.value() >> 32) as u32));
        self.dbr_addr_l.write(U32::new(db_addr.value() as u32));
        let log_wq_stride = libm::log2(64.0) as u32; //=64 ?????
        let log_wq_page_size = libm::log2(libm::ceil((2_usize.pow(log_wq_size as u32) * 64) as f64 / PAGE_SIZE as f64)) as u32;
        self.log_wq_stride_pg_sz_sz.write(U32::new((log_wq_stride << 16) | (log_wq_page_size << 8) | (log_wq_size as u32 & 0x1F)));
    }
}

enum WQEOpcode {
    Nop = 0x0,
    SndInv = 0x1,
    RDMAWrite = 0x8,
    RDMAWriteWithImmediate = 0x9,
    Send = 0xA,
    SendWithImmediate = 0xB,
    LargeSendOffload = 0xE,
    Wait = 0xF,
    RDMARead = 0x10,
    AtomicCompareAndSwap = 0x11,
    AtomicFetchAndAdd = 0x12,
    AtomicMaskedCompareAndSwap = 0x14,
    AtomicMaskedFetchAndAdd = 0x15,
    ReceiveEn = 0x16,
    SendEn = 0x17,
    SetPsv = 0x20,
    Dump = 0x23,
    Umr = 0x25 
}

enum SendOpMod {
    None = 0x0,
    VectorCalcSegment = 0xFF
}

#[derive(FromBytes, Default)]
#[repr(C)]
pub(crate) struct WorkQueueEntry {
    pub(crate) control: ControlSegment,
    eth: EthSegment,
    data: MemoryPointerDataSegment
}

const_assert_eq!(core::mem::size_of::<WorkQueueEntry>(), 64);

impl WorkQueueEntry {
    pub fn init(&mut self) {
        *self = WorkQueueEntry::default();
    }
    pub fn init_send(&mut self, wqe_index: u32, sqn: u32, tisn: u32, lkey: u32, local_address: PhysicalAddress) {
        self.control.init(wqe_index, sqn, tisn);
        self.eth.init(local_address);
        self.data.init(lkey, local_address);
    }

    pub fn nop(&mut self, wqe_index: u32, sqn: u32, tisn: u32, lkey: u32) {
        self.control.nop(wqe_index, sqn, tisn);
    }

    pub fn dump(&self, i: usize) {
        debug!("WQE {}", i);
        unsafe {
            let ptr = self as *const WorkQueueEntry as *const u32;
            debug!("{:#010x} {:#010x} {:#010x} {:#010x}", (*ptr).to_be(), (*ptr.offset(1)).to_be(), (*ptr.offset(2)).to_be(), (*ptr.offset(3)).to_be());
            debug!("{:#010x} {:#010x} {:#010x} {:#010x}", (*ptr.offset(4)).to_be(), (*ptr.offset(5)).to_be(), (*ptr.offset(6)).to_be(), (*ptr.offset(7)).to_be());
            debug!("{:#010x} {:#010x} {:#010x} {:#010x}", (*ptr.offset(8)).to_be(), (*ptr.offset(9)).to_be(), (*ptr.offset(10)).to_be(), (*ptr.offset(11)).to_be());
            debug!("{:#010x} {:#010x} {:#010x} {:#010x} \n", (*ptr.offset(12)).to_be(), (*ptr.offset(13)).to_be(), (*ptr.offset(14)).to_be(), (*ptr.offset(15)).to_be());
        }
    }
}

#[derive(FromBytes, Default)]
#[repr(C)]
pub(crate) struct ControlSegment {
    pub(crate) opcode:             Volatile<U32<BigEndian>>,
    pub(crate) ds:                 Volatile<U32<BigEndian>>,
    se:                 Volatile<U32<BigEndian>>,
    ctrl_general_id:    Volatile<U32<BigEndian>>,
}

const_assert_eq!(core::mem::size_of::<ControlSegment>(), 16);

impl ControlSegment {
    pub fn init(&mut self, wqe_index: u32, sqn: u32, tisn: u32) {
        self.opcode.write(U32::new((wqe_index << 8)| (WQEOpcode::Send as u32)));
        self.ds.write(U32::new((sqn << 8) | 4));
        self.se.write(U32::new(8));
        // self.ctrl_general_id.write(U32::new(tisn << 8)); //?
    }

    pub fn nop(&mut self, wqe_index: u32, sqn: u32, tisn: u32) {
        self.opcode.write(U32::new((wqe_index << 8)| (WQEOpcode::Nop as u32)));
        debug!("{:#X}", (sqn << 8) | 4);
        self.ds.write(U32::new((sqn << 8) | 4));
        self.se.write(U32::new(8));
        // self.ctrl_general_id.write(U32::new(tisn << 8)); //?
    }
}


#[derive(FromBytes, Default)]
#[repr(C)]
pub(crate) struct EthSegment {
    _padding0:              u32,
    mss:                    Volatile<U32<BigEndian>>,
    flow_table_metadata:    Volatile<U32<BigEndian>>,
    inline_headers_0:       Volatile<U32<BigEndian>>,
    inline_headers_1:       Volatile<U32<BigEndian>>,
    inline_headers_2:       Volatile<U32<BigEndian>>,
    inline_headers_3:       Volatile<U32<BigEndian>>,
    inline_headers_4:       Volatile<U32<BigEndian>>,
}

const_assert_eq!(core::mem::size_of::<EthSegment>(), 32);

impl EthSegment {
    pub fn init(&mut self, packet: PhysicalAddress) {
        let inline_headers_0 = (14 << 16) /* bytes in ethernet header*/ | 0xFFFF;
        let inline_headers_1 = 0xFFFF_FFFF;  
        let inline_headers_2 = 0x043f_72a2;  // 04:3f:72:a2:b4:3a
        let inline_headers_3 = (300 << 16) | 0xb43a;  // 04:3f:72:a2:b4:3a
        let inline_headers_4 = 0x4500;  // 

        self.inline_headers_0.write(U32::new(inline_headers_0));
        self.inline_headers_1.write(U32::new(inline_headers_1));
        self.inline_headers_2.write(U32::new(inline_headers_2));
        self.inline_headers_3.write(U32::new(inline_headers_3));
        self.inline_headers_4.write(U32::new(inline_headers_4));

    }
}

#[derive(FromBytes, Default)]
#[repr(C)]
pub(crate) struct MemoryPointerDataSegment {
    byte_count:         Volatile<U32<BigEndian>>,
    l_key:              Volatile<U32<BigEndian>>,
    local_address_h:    Volatile<U32<BigEndian>>,
    local_address_l:    Volatile<U32<BigEndian>>,
}

const_assert_eq!(core::mem::size_of::<MemoryPointerDataSegment>(), 16);

impl MemoryPointerDataSegment {
    pub fn init(&mut self, lkey: u32, local_address: PhysicalAddress) {
        self.byte_count.write(U32::new(298));
        self.l_key.write(U32::new(lkey));
        self.local_address_h.write(U32::new((local_address.value() >> 32) as u32));
        self.local_address_l.write(U32::new((local_address.value() & 0xFFFF_FFFF) as u32));
    }
}

