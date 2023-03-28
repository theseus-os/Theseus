//! The Work Queue (WQ) contains a contiguous memory buffer used by SW to post I/O requests (WQEs) for HCA execution.
//! A Work Request is posted to the HCA by writing to one or more Work Queue Elements (WQE) of the WQ and
//! ringing the DoorBell, notifying the HCA that request has been posted.
//! A WQ is created for every SQ and RQ and is comprised of WQE Basic Blocks (WQEBBs) which are 64 byte units.
//! 
//! This module defines the context used to initialize a WQ, layout of WQ Doorbell Records, the layout of WQEBBs and related functions.
//! 
//! (PRM Section 8.8: Work Queues)
//! 
use zerocopy::{U32, FromBytes};
use volatile::{ReadOnly, Volatile};
use byteorder::BigEndian;
use memory::PhysicalAddress;
use num_enum::TryFromPrimitive;
use crate::log_page_size;

/// The layout of a doorbell record in memory.
/// A doorbell should be created for each SQ/RQ pair, and its address passed to the HW at time of SQ/RQ creation.
/// 
/// (PRM Section 8.8.2: Doorbell Record)
#[derive(FromBytes, Default)]
#[repr(C)]
pub struct DoorbellRecord {
    /// Receive Counter (aka wqe_counter).
    /// This counter stores the number of receive WQEs posted since creation.
    pub(crate) rcv_counter:    Volatile<U32<BigEndian>>,
    /// Send Counter (aka sq_wqebb_counter).
    /// This counter stores the number of send WQEs posted since creation.
    pub(crate) send_counter:   Volatile<U32<BigEndian>>,
}

const _: () = assert!(core::mem::size_of::<DoorbellRecord>() == 8);

/// The possible formats for a WQ buffer.
#[derive(Debug, TryFromPrimitive)]
#[repr(u32)]
enum WQType {
    LinkedList = 0,
    Cyclic = 1,
    LinkedListStriding = 2,
    CyclicStriding = 3
}

/// A struct representing the layout of a WQ in memory.
/// A WQ is part of the SQ and RQ context. 
/// 
/// (PRM Table 109: Work Queue Format)
#[derive(FromBytes, Default)]
#[repr(C)]
pub(crate) struct WorkQueue {
    /// A multi-part field:
    /// * `wq_type`: a value of [`WQType`], occupies bits [31:28]
    /// * `wq_signature`: if set, WQE signature will be checked on this WQ, occupies bit 27
    /// * `end_padding_mode`: if incoming packet should be padded, occupies bits [26:25]
    /// * `cd_slave`: if set, WQ is a recipient of CD doorbells via the master SQ, occupies bit 24
    wq_type_signature:                  Volatile<U32<BigEndian>>,
    /// A multi-part field:
    /// * `page_offset`: page offset in quanta of (page_size / 64), occupies bits [20:16]
    /// * `lwm`: limit water mark (disabled when 0), when WQE count drops below this limit, an event is fired, occupies bits [15:0]
    page_offset_lwm:                    Volatile<U32<BigEndian>>,
    /// protection domain, occupies bits [23:0]
    pd:                                 Volatile<U32<BigEndian>>,
    /// UAR number allocated for ringing DoorBells for this WQ, occupies bits [23:0]
    uar_page:                           Volatile<U32<BigEndian>>,
    /// upper 4 bytes of physical address of DB Record
    dbr_addr_h:                         Volatile<U32<BigEndian>>,
    /// lower 4 bytes of physical address of DB Record
    dbr_addr_l:                         Volatile<U32<BigEndian>>,
    /// current HW stride index, points to the next stride to be consumed by HW
    hw_counter:                         ReadOnly<U32<BigEndian>>,
    /// current SW WQ WQE index, points to the next stride to be produced by SW
    sw_counter:                         ReadOnly<U32<BigEndian>>,
    /// A multi-part field:
    /// * `log_wq_stride`: the size of a WQ stride equals 2^log_wq_stride, occupies bits [19:16]
    /// * `log_wq_pg_sz`: log (base 2) of page size in units of 4KiB, occupies bits [12:8]
    /// * `log_wq_sz`: log (base 2) of the WQ size (in entries), occupies bits [4:0] TODO: check again
    log_wq_stride_pg_sz_sz:             Volatile<U32<BigEndian>>,
    single_stride_log_num_of_bytes:     Volatile<U32<BigEndian>>,
    _padding1:                          [u64; 19],
}

const _: () = assert!(core::mem::size_of::<WorkQueue>() == 192);

impl WorkQueue {
    /// Create and initialize the fields of the WQ for a SQ or RQ context.
    /// This is then passed to the HCA as part of the Context when creating the queue.
    /// 
    /// # Arguments
    /// * `pd`: protection domain number
    /// * `db_addr`: physical address of the doorbell record
    /// * `wq_size`: number of WQ entries
    /// * `wqe_size_in_bytes`: size of the WQE
    fn init(pd: u32, db_addr: PhysicalAddress, wq_size: u32, wqe_size_in_bytes: u32) -> WorkQueue {
        const WQ_TYPE_SHIFT: u32 = 28;
        const PD_MASK: u32 = 0xFF_FFFF;
        const WQ_STRIDE_SHIFT: u32 = 16;
        const WQ_PAGE_SIZE_SHIFT: u32 = 8;

        // set all fields to zero
        let mut wq = WorkQueue::default();
        
        wq.wq_type_signature.write(U32::new((WQType::Cyclic as u32) << WQ_TYPE_SHIFT)); 
        wq.pd.write(U32::new(pd & PD_MASK));
        wq.dbr_addr_h.write(U32::new((db_addr.value() >> 32) as u32));
        wq.dbr_addr_l.write(U32::new(db_addr.value() as u32));
        
        // the stride of the WQE is equal to the size of the WQE
        let log_wq_stride = libm::log2(wqe_size_in_bytes as f64) as u32; 
        let log_wq_size = libm::log2(wq_size as f64) as u32;
        let log_wq_page_size = log_page_size(wq_size * wqe_size_in_bytes);
        wq.log_wq_stride_pg_sz_sz.write(U32::new((log_wq_stride << WQ_STRIDE_SHIFT) | (log_wq_page_size << WQ_PAGE_SIZE_SHIFT) | log_wq_size));
        wq
    }

    /// Create and initialize the fields of the WQ for a SQ context.
    /// This is then passed to the HCA as part of the SQ Context when creating the SQ.
    /// 
    /// # Arguments
    /// * `pd`: protection domain number
    /// * `uar_page`: UAR page number (only provided for a SQ)
    /// * `db_addr`: physical address of the doorbell record
    /// * `wq_size`: number of WQ entries
    pub fn init_sq(pd: u32, uar_page: u32, db_addr: PhysicalAddress, wq_size: u32) -> WorkQueue {
        const UAR_PAGE_MASK: u32 = 0xFF_FFFF;

        let mut wq = Self::init(pd, db_addr, wq_size, core::mem::size_of::<WorkQueueEntrySend>() as u32);
        wq.uar_page.write(U32::new(uar_page & UAR_PAGE_MASK));
        wq
    }

    pub fn init_rq(pd: u32, db_addr: PhysicalAddress, wq_size: u32) -> WorkQueue {
        Self::init(pd, db_addr, wq_size, core::mem::size_of::<WorkQueueEntryReceive>() as u32)
    }

    /// Offset that this context is written to in the mailbox buffer
    pub(crate) fn mailbox_offset() -> usize { 0x10 + 0x30 }
}

/// The possible values of the opcode field in the control segment of a WQE
#[derive(Debug, TryFromPrimitive)]
#[repr(u8)]
pub(crate) enum WQEOpcode {
    /// WQE with this opcode creates a completion, but does nothing else
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

/// WQEs are built from multiple segments.
/// In the case of Send WQEs, there are three:
/// * control segment
/// * eth segment
/// * memory pointer data segment
#[derive(FromBytes, Default)]
#[repr(C)]
pub struct WorkQueueEntrySend {
    /// This segment contains control information of the WQE
    pub(crate) control: ControlSegment,
    /// This segment contains inlined Ethernet packet headers
    eth: EthSegment,
    /// This segment contains the length and address of the packet buffer
    data: MemoryPointerDataSegment
}

const _: () = assert!(core::mem::size_of::<WorkQueueEntrySend>() == 64);

impl WorkQueueEntrySend {
    /// set a WQE to an initial state
    pub fn init(&mut self) {
        *self = WorkQueueEntrySend::default();
    }

    /// Fill the control, ethernet and data segments of the WQE to send packets.
    /// 
    /// # Arguments    
    /// * `wqe_index`: WQEBB number of the first block of this WQE // TODO? seems to be wqe_counter
    /// * `sqn`: number of the SQ this WQE is posted to
    /// * `lkey`: the lkey used by the SQ
    /// * `local_address`: physical address of the packet buffer
    /// * `packet`: packet buffer
    pub fn send(&mut self, wqe_index: u32, sqn: u32, lkey: u32, local_address: PhysicalAddress, packet: &[u8]) {
        self.control.send(wqe_index, sqn);
        self.eth.init(packet);
        self.data.init(lkey, local_address, packet.len() as u32);
    }

    /// Fill the control segment of the WQE to execute a NOP.
    /// 
    /// # Arguments    
    /// * `wqe_index`: WQEBB number of the first block of this WQE
    /// * `sqn`: number of the SQ this WQE is posted to
    pub fn nop(&mut self, wqe_index: u32, sqn: u32) {
        self.control.nop(wqe_index, sqn);
    }

    /// Prints out the fields of a WQE in the format used by other drivers (e.g. Linux, Snabb)
    pub fn dump(&self, i: usize) {
        debug!("Tx WQE {}", i);
        unsafe {
            let ptr = self as *const WorkQueueEntrySend as *const u32;
            debug!("{:#010x} {:#010x} {:#010x} {:#010x}", (*ptr).to_be(), (*ptr.offset(1)).to_be(), (*ptr.offset(2)).to_be(), (*ptr.offset(3)).to_be());
            debug!("{:#010x} {:#010x} {:#010x} {:#010x}", (*ptr.offset(4)).to_be(), (*ptr.offset(5)).to_be(), (*ptr.offset(6)).to_be(), (*ptr.offset(7)).to_be());
            debug!("{:#010x} {:#010x} {:#010x} {:#010x}", (*ptr.offset(8)).to_be(), (*ptr.offset(9)).to_be(), (*ptr.offset(10)).to_be(), (*ptr.offset(11)).to_be());
            debug!("{:#010x} {:#010x} {:#010x} {:#010x} \n", (*ptr.offset(12)).to_be(), (*ptr.offset(13)).to_be(), (*ptr.offset(14)).to_be(), (*ptr.offset(15)).to_be());
        }
    }
}

/// WQEs are built from multiple segments.
/// In the case of Receive WQEs, there is only the memory pointer data segment
#[derive(FromBytes, Default)]
#[repr(C)]
pub struct WorkQueueEntryReceive {
    /// This segment contains the length and address of the packet buffer
    data: MemoryPointerDataSegment
}

const _: () = assert!(core::mem::size_of::<WorkQueueEntryReceive>() == 16);

impl WorkQueueEntryReceive {
    /// set a WQE to an initial state
    pub fn init(&mut self) {
        *self = WorkQueueEntryReceive::default();
    }

    /// Fill the data segment of the WQE to receive packets.
    /// 
    /// # Arguments    
    /// * `lkey`: the lkey used by the RQ
    /// * `local_address`: physical address of the packet buffer
    /// * `packet_len`: packet buffer length in bytes
    pub fn update_buffer_info(&mut self, lkey: u32, local_address: PhysicalAddress, packet_len: u32) {
        self.data.init(lkey, local_address, packet_len);
    }

    /// Prints out the fields of a WQE in the format used by other drivers (e.g. Linux, Snabb)
    pub fn dump(&self, i: usize) {
        debug!("Rx WQE {}", i);
        unsafe {
            let ptr = self as *const WorkQueueEntryReceive as *const u32;
            debug!("{:#010x} {:#010x} {:#010x} {:#010x}", (*ptr).to_be(), (*ptr.offset(1)).to_be(), (*ptr.offset(2)).to_be(), (*ptr.offset(3)).to_be());
        }
    }
}


/// Possible values of the CE subfield in the [`ControlSegment`] 
#[derive(Debug, TryFromPrimitive)]
#[repr(u8)]
enum CompletionAndEventMode {
    /// Generate CQE only on WQE completion with error
    CQEOnWQEError = 0,
    /// Generate CQE only on first WQE completion with error
    CQEOnFirstWQEError = 1,
    /// Generate CQE on WQE completion
    CQEAlways = 2,
    /// Generate CQE and EQE
    CQEAndEQE = 3
}

/// This segment contains control information of the WQE.
#[derive(FromBytes, Default)]
#[repr(C)]
pub(crate) struct ControlSegment {
    /// A multi-part field:
    /// * `opc_mod`: opcode modifier, occupies bits [31:24]
    /// * `wqe_index`: WQEBB number of the first block of this WQE, occupies bits [23:8]
    /// * `opcode`: a value of the type [`WQEOpcode`], occupies bits [7:0]
    pub(crate) opcode:              Volatile<U32<BigEndian>>,
    /// A multi-part field:
    /// * `qp_or_sq`: QP/SQ number this WQE is posted to, occupies bits [31:8]
    /// * `ds`: WQE size in octowords (16-byte units), occupies bits [5:0]
    pub(crate) ds:                  Volatile<U32<BigEndian>>,
    /// A multi-part field:
    /// * `ce`: A value of the type [`CompletionAndEventMode`], occupies bits [3:2]
    /// * `se`: true if a solicited event, occupies bit 1
    ce_se:                          Volatile<U32<BigEndian>>,
    /// general identifier according to WQE opcode/opc_mod
    ctrl_general_id:                Volatile<U32<BigEndian>>,
}

const _: () = assert!(core::mem::size_of::<ControlSegment>() == 16);

impl ControlSegment {
    /// Initialize the fields of the control segment.
    /// 
    /// # Arguments
    /// * `opcode`: the type of command this WQE will complete
    /// * `wqe_index`: WQEBB number of the first block of this WQE
    /// * `sqn`: number of the SQ this WQE is posted to
    fn init(&mut self, opcode: WQEOpcode, wqe_index: u32, sqn: u32) {
        const WQE_INDEX_SHIFT: u32 = 8;
        const SQN_SHIFT: u32 = 8;
        // WQE size in octowords (16-byte units)
        const WQE_SIZE_IN_OCTWORDS: u32 =  core::mem::size_of::<WorkQueueEntrySend>() as u32 / 16;
        const CE_SHIFT: u32 = 2;

        self.opcode.write(U32::new((wqe_index << WQE_INDEX_SHIFT)| (opcode as u32)));
        self.ds.write(U32::new((sqn << SQN_SHIFT) | WQE_SIZE_IN_OCTWORDS));
        self.ce_se.write(U32::new((CompletionAndEventMode::CQEAlways as u32) << CE_SHIFT));
    }

    /// Initialize the fields of the control segment to send a packet.
    /// 
    /// # Arguments
    /// * `wqe_index`: WQEBB number of the first block of this WQE
    /// * `sqn`: number of the SQ this WQE is posted to
    pub fn send(&mut self, wqe_index: u32, sqn: u32) {
        self.init(WQEOpcode::Send, wqe_index, sqn)
    }

    /// Initialize the fields of the control segment to create a NOP.
    /// With a NOP, only a completion event will be generated. 
    /// It's a good way to make sure the CQ and WQ are properly initialized.
    /// 
    /// # Arguments
    /// * `wqe_index`: WQEBB number of the first block of this WQE
    /// * `sqn`: number of the SQ this WQE is posted to
    pub fn nop(&mut self, wqe_index: u32, sqn: u32) {
        self.init(WQEOpcode::Nop, wqe_index, sqn)
    }
}

/// This segment contains stateless offloading control and inlined Ethernet packet headers
#[derive(FromBytes, Default)]
#[repr(C)]
pub(crate) struct EthSegment {
    _padding0:              u32,
    /// Maximum Segment Size
    mss:                    Volatile<U32<BigEndian>>,
    flow_table_metadata:    Volatile<U32<BigEndian>>,
    /// A multi-part field:
    /// * `inline_header_size`: length of inlined packet headers in bytes, occupies bits [25:16]
    /// * `inline_headers`: beginning of the inlined packet headers, occupies bits [15:0]
    inline_headers_0:       Volatile<U32<BigEndian>>,
    /// bytes 2 to 5 of the packet header
    inline_headers_1:       Volatile<U32<BigEndian>>,
    /// bytes 6 to 9 of the packet header
    inline_headers_2:       Volatile<U32<BigEndian>>,
    /// bytes 10 to 13 of the packet header
    inline_headers_3:       Volatile<U32<BigEndian>>,
    /// bytes 14 and 15 of the packet header
    inline_headers_4:       Volatile<U32<BigEndian>>,
}

const _: () = assert!(core::mem::size_of::<EthSegment>() == 32);

impl EthSegment {
    /// Initialize the fields of the eth segment to send a packet.
    /// 
    /// # Arguments
    /// * `packet`: packet buffer
    pub fn init(&mut self, packet: &[u8]) {
        const INLINE_HEADER_SIZE: u32 = 16;
        const INLINE_HEADER_SHIFT: u32 = 16;

        self.inline_headers_0.write(U32::new((INLINE_HEADER_SIZE << INLINE_HEADER_SHIFT) | (packet[0] as u32) << 8 | (packet[1] as u32)));
        self.inline_headers_1.write(U32::new((packet[2] as u32) << 24 | (packet[3] as u32) << 16 | (packet[4] as u32) << 8 | packet[5] as u32));
        self.inline_headers_2.write(U32::new((packet[6] as u32) << 24 | (packet[7] as u32) << 16 | (packet[8] as u32) << 8 | packet[9] as u32));
        self.inline_headers_3.write(U32::new((packet[10] as u32) << 24 | (packet[11] as u32) << 16 | (packet[12] as u32) << 8 | packet[13] as u32));
        self.inline_headers_4.write(U32::new((packet[14] as u32) << 24 | (packet[15] as u32) << 16));
    }
}

/// This segment contains the length and address of the packet buffer
#[derive(FromBytes, Default)]
#[repr(C)]
pub(crate) struct MemoryPointerDataSegment {
    /// length of the packet in bytes
    byte_count:         Volatile<U32<BigEndian>>,
    /// the lkey used by the WQ
    l_key:              Volatile<U32<BigEndian>>,
    /// upper 4 bytes of the physical address of the packet buffer
    local_address_h:    Volatile<U32<BigEndian>>,
    /// lower 4 bytes of the physical address of the packet buffer
    local_address_l:    Volatile<U32<BigEndian>>,
}

const _: () = assert!(core::mem::size_of::<MemoryPointerDataSegment>() == 16);

impl MemoryPointerDataSegment {
    /// Initialize the fields of the data segment to send or receive a packet.
    /// 
    /// # Arguments    
    /// * `lkey`: the lkey used by the WQ
    /// * `local_address`: physical address of the packet buffer
    /// * `len`: length of the packet in bytes
    pub fn init(&mut self, lkey: u32, local_address: PhysicalAddress, len: u32) {
        self.byte_count.write(U32::new(len));
        self.l_key.write(U32::new(lkey));
        self.local_address_h.write(U32::new((local_address.value() >> 32) as u32));
        self.local_address_l.write(U32::new((local_address.value() & 0xFFFF_FFFF) as u32));
    }
}
