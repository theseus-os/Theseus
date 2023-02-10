//! The flow table sets rules for forwarding packets to different queues.
//! This module defines the layout of the context used to initialize a flow table.

use zerocopy::{U32, FromBytes};
use volatile::Volatile;
use byteorder::BigEndian;
use core::fmt;
use num_enum::TryFromPrimitive;

/// Value written to the flow table context to set the flow table's role in packet processing.
/// (PRM Section 23.17.1, Table 1737)
#[derive(Debug, TryFromPrimitive)]
#[repr(u8)]
pub(crate) enum FlowTableType {
    NicRx = 0x0,
    NicTx = 0x1
}

/// The data structure containing flow table initialization parameters.
/// It is passed to the HCA at the time of flow table creation.
/// (Many fields are missing since they are not used)
/// (PRM Section 23.17.1, Table 1740)
#[derive(FromBytes, Default)]
#[repr(C, packed)]
pub(crate) struct FlowTableContext {
    /// log base 2 of the table size (given in the number of flows), occupies bits [7:0]
    pub(crate) log_size:                    Volatile<U32<BigEndian>>,
}

const _: () = assert!(core::mem::size_of::<FlowTableContext>() == 4);

impl fmt::Debug for FlowTableContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("FlowTableContext")
            .field("log_size", &self.log_size.read().get())
            .finish()
    }
}

impl FlowTableContext {
    pub(crate) fn init(num_entries: u32) -> FlowTableContext {
        let mut ctxt = FlowTableContext::default();
        let log_size = libm::log2(num_entries as f64) as u32;
        ctxt.log_size.write(U32::new(log_size));
        ctxt
    }

    /// Offset that this context is written to in the mailbox buffer
    pub(crate) fn mailbox_offset() -> usize { 0x8 }
}

/// Value written to the [`FlowGroupInput`] match_criteria_enable bitmask field.
/// (PRM Section 23.17.6, Table 1759)
#[derive(Debug, TryFromPrimitive)]
#[repr(u8)]
pub(crate) enum MatchCriteriaEnable {
    /// Use this option for the wildcard flow group
    None                = 0,
    OuterHeaders        = 1 << 0,
    MiscParameters      = 1 << 1,
    InnerHeader         = 1 << 2
}

/// The data structure containing flow group initialization parameters.
/// It is passed to the HCA at the time of flow group creation.
/// (Many fields are missing since they are not used)
/// (PRM Section 23.17.6, Table 1758)
#[derive(FromBytes, Default)]
#[repr(C, packed)]
pub(crate) struct FlowGroupInput {
    /// The table's role in packet processing, occupies bits [31:24]
    pub(crate) table_type:                      Volatile<U32<BigEndian>>,
    /// Table handler returned by the [`CommandOpcode::CreateFlowTable`] command
    pub(crate) table_id:                        Volatile<U32<BigEndian>>,
    _padding1:                                  u32,
    /// The first flow included in the group
    pub(crate) start_flow_index:                Volatile<U32<BigEndian>>,
    _padding2:                                  u32,
    /// The last flow included in the group
    pub(crate) end_flow_index:                  Volatile<U32<BigEndian>>,
    _padding3:                                  [u8; 20],
    /// Bitmask representing which of the header and parameters in the
    /// match_criteria field are used in defining the flow.
    pub(crate) match_criteria_enable:           Volatile<U32<BigEndian>>,
}

const _: () = assert!(core::mem::size_of::<FlowGroupInput>() == 48);

impl FlowGroupInput {
    pub(crate) fn init(
        table_type: FlowTableType, 
        table_id: u32, 
        start_flow_index: u32, 
        end_flow_index: u32, 
        match_criteria_enable: MatchCriteriaEnable
    ) -> FlowGroupInput {
        let mut fgi = FlowGroupInput::default();
        fgi.table_type.write(U32::new((table_type as u32) << 24));
        fgi.table_id.write(U32::new(table_id & 0xFF_FFFF));
        fgi.start_flow_index.write(U32::new(start_flow_index));
        fgi.end_flow_index.write(U32::new(end_flow_index));
        fgi.match_criteria_enable.write(U32::new(match_criteria_enable as u32)); 
        fgi
    }

    /// Offset that this context is written to in the mailbox buffer
    pub(crate) fn mailbox_offset() -> usize { 0 }
}

impl fmt::Debug for FlowGroupInput {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("FlowTableContext")
            .field("table_type", &self.table_type.read().get())
            .field("table_id", &self.table_id.read().get())
            .field("start_flow_index", &self.start_flow_index.read().get())
            .field("end_flow_index", &self.end_flow_index.read().get())
            .field("match_criteria_enable", &self.match_criteria_enable.read().get())
            .finish()
    }
}

/// The data structure containing information to add an entry to a flow table.
/// (PRM Section 23.17.9, Table 1788)
#[derive(FromBytes, Default)]
#[repr(C, packed)]
pub(crate) struct FlowEntryInput {
    /// The table's role in packet processing, occupies bits [31:24]
    pub(crate) table_type:                  Volatile<U32<BigEndian>>,
    /// Table handler returned by the [`CommandOpcode::CreateFlowTable`] command
    pub(crate) table_id:                    Volatile<U32<BigEndian>>,
    _padding1:                              [u8; 8],
    /// flow index in the flow table
    pub(crate) flow_index:                  Volatile<U32<BigEndian>>,
    _padding2:                              [u8; 28]
}

const _: () = assert!(core::mem::size_of::<FlowEntryInput>() == 48);

impl FlowEntryInput {
    pub(crate) fn init(table_type: FlowTableType, table_id: u32, flow_index: u32) -> FlowEntryInput {
        let mut fei = FlowEntryInput::default();
        fei.table_type.write(U32::new((table_type as u32) << 24));
        fei.table_id.write(U32::new(table_id & 0xFF_FFFF));
        fei.flow_index.write(U32::new(flow_index));
        fei
    }

    /// Offset that this context is written to in the mailbox buffer
    pub(crate) fn mailbox_offset() -> usize { 0 }
}

/// Value written to the [`FlowContext`] action bitmask field.
/// (PRM Section 23.17.9, Table 1791)
#[derive(Debug, TryFromPrimitive)]
#[repr(u32)]
pub(crate) enum FlowContextAction {
    None            = 0,
    Allow           = 1 << 0,
    Drop            = 1 << 1,
    FwdDest         = 1 << 2,
    Count           = 1 << 3,
    Reformat        = 1 << 4,
    Decap           = 1 << 5,
}


/// The data structure containing information about a flow.
/// (PRM Section 23.17.9, Table 1790)
#[derive(FromBytes)]
#[repr(C, packed)]
pub(crate) struct FlowContext {
    _padding1:                              u32,
    /// Group handler returned by the [`CommandOpcode::CreateFlowGroup`] command
    pub(crate) group_id:                    Volatile<U32<BigEndian>>,
    _padding2:                              u32,
    /// bitmask indicating which actions to perform
    pub(crate) action:                      Volatile<U32<BigEndian>>,
    /// size of destination list
    pub(crate) dest_list_size:              Volatile<U32<BigEndian>>,
    _padding3:                              [u8; 20] 
}

const _: () = assert!(core::mem::size_of::<FlowContext>() == 40);

impl Default for FlowContext {
    fn default() -> FlowContext {
        FlowContext { 
            _padding1: 0, 
            group_id: Volatile::new(U32::new(0)), 
            _padding2: 0, 
            action: Volatile::new(U32::new(0)),  
            dest_list_size: Volatile::new(U32::new(0)),  
            _padding3: [0; 20] 
        }
    }
}
impl FlowContext {
    pub(crate) fn init(group_id: u32, action: FlowContextAction, dest_list_size: u32) -> FlowContext {
        let mut ctxt = FlowContext::default();
        ctxt.group_id.write(U32::new(group_id));
        ctxt.action.write(U32::new(action as u32));
        ctxt.dest_list_size.write(U32::new(dest_list_size)); 
        ctxt
    }

    /// Offset that this context is written to in the mailbox buffer
    pub(crate) fn mailbox_offset() -> usize { 0x30 }
}


/// Value written to the [`DestinationEntry`] id_and_type field.
/// (PRM Section 23.17.9, Table 1801)
#[derive(Debug, TryFromPrimitive)]
#[repr(u8)]
pub(crate) enum DestinationType {
    VPort           = 0x0,
    FlowTable       = 0x1,
    Tir             = 0x2,
    Qp              = 0x3,
    FlowSampler     = 0x6,
}


/// The data structure containing information about the destination of a flow.
/// (PRM Section 23.17.9, Table 1800)
#[derive(FromBytes, Default)]
#[repr(C, packed)]
pub(crate) struct DestinationEntry {
    /// currently we only set the type to [DestinationType::Tir] and the id is the TIR number for the RQ
    pub(crate) id_and_type:                Volatile<U32<BigEndian>>,
    pub(crate) packet_reformat:                 Volatile<U32<BigEndian>>,

}

const _: () = assert!(core::mem::size_of::<DestinationEntry>() == 8);

impl DestinationEntry {
    pub(crate) fn init(dest_type: DestinationType, dest_id: u32) -> DestinationEntry {
        let mut entry = DestinationEntry::default();
        entry.id_and_type.write(U32::new((dest_type as u32) << 24 | (dest_id & 0xFF_FFFF)));
        entry
    }
}
