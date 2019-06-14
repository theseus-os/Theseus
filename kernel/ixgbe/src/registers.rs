use volatile::{Volatile, ReadOnly, WriteOnly};

/// Vendor ID for Intel
pub const INTEL_VEND:                   u16 = 0x8086;  

/// Device ID for the 82599ES, used to identify the device from the PCI space
pub const INTEL_82599:                  u16 = 0x10FB;  

/// The memory mapped registers of the 82599ES device
#[repr(C)]
pub struct IntelIxgbeRegisters {
    /// Device Control Register
    pub ctrl:                           Volatile<u32>,          // 0x0
    _padding0:                          [u8; 4],                // 0x4 - 0x7
    
    /// Device Status Register
    pub status:                         ReadOnly<u32>,          // 0x8
    _padding1:                          [u8; 28],               // 0xC - 0x27

    /// I2C Control
    pub i2cctl:                         Volatile<u32>,          // 0x28
    _padding2:                          [u8; 2004],             // 0x2C - 0x7FF

    /// Extended Interrupt Cause Register
    pub eicr:                           Volatile<u32>,          // 0x800
    _padding3:                          [u8; 4],                // 0x804 - 0x807

    /// Extended Interrupt Cause Set Register
    pub eics:                           WriteOnly<u32>,         // 0x808
    _padding4:                          [u8; 4],                // 0x80C - 0x80F

    /// Extended Interrupt Auto Clear Register
    pub eiac:                           Volatile<u32>,          // 0x810; 
    _padding5:                          [u8; 12],               // 0x814 - 0x81F 
    
    /// Extended Interrupt Throttle Registers
    pub eitr:                           RegisterArray24,        // 0x820 - 0x87F; 

    /// Extended Interrupt Mask Set/ Read Register
    pub eims:                           Volatile<u32>,          // 0x880; 
    _padding6:                          [u8; 4],                // 0x884 - 0x887

    /// Extended Interrupt Mask Clear Register
    pub eimc:                           WriteOnly<u32>,         // 0x888; 
    _padding7:                          [u8; 4],                // 0x88C - 0x88F     

    /// Extended Interrupt Auto Mask Enable Register
    pub eiam:                           Volatile<u32>,          // 0x890; 
    _padding8:                          [u8; 4],                // 0x894 - 0x897

    /// General Purpose Interrupt Enable
    pub gpie:                           Volatile<u32>,          // 0x898; 
    _padding9:                          [u8; 100],              // 0x89C - 0x8FF

    /// Interrupt Vector Allocation Registers
    pub ivar:                           RegisterArray64,        // 0x900 - 0x9FF  
    _padding10:                         [u8; 1536],             // 0xA00 - 0xFFF

    /// First set of Rx Registers for 64 Rx Queues
    pub rx_regs1:                       RegisterArrayRx,        // 0x1000 - 0x1FFF
    _padding11:                         [u8; 3840],             // 0x2000 - 0x2EFF

    /// Receive DMA Control Register
    pub rdrxctl:                        Volatile<u32>,          // 0x2F00;
    _padding12:                         [u8; 252],              // 0x2F04 - 0x2FFF

    /// Receive Control Register
    pub rxctrl:                         Volatile<u32>,          // 0x3000;
    _padding13:                         [u8; 508],              // 0x3004 - 0x31FF

    /// Flow Control Transmit Timer Value
    pub fcttv:                          RegisterArray4,         // 0x3200 - 0x320F
    _padding14:                         [u8; 16],               // 0x3210 - 0x321F

    /// Flow Control Receive Threshold Low
    pub fcrtl:                          RegisterArray8,         // 0x3220 - 0x323F
    _padding15:                         [u8; 32],               // 0x3240 - 0x325F 

    /// Flow Control Receive Threshold High
    pub fcrth:                          RegisterArray8,         // 0x3260 - 0x327F
    _padding16:                         [u8; 32],               // 0x3280 - 0x329F

    /// Flow Control Refresh Threshold Value
    pub fcrtv:                          Volatile<u32>,          // 0x32A0;
    _padding17:                         [u8; 2652],             // 0x32A4 - 0x3CFF

    /// Flow Control Configuration
    pub fccfg:                          Volatile<u32>,          // 0x3D00;
    _padding18:                         [u8; 1340],             // 0x3D04 - 0x423F

    /// MAC Core Control 0 Register 
    pub hlreg0:                         Volatile<u32>,          // 0x4240;
    _padding19:                         [u8; 92],               // 0x4244 - 0x429F

    /// Auto-Negotiation Control Register
    pub autoc:                          Volatile<u32>,          // 0x42A0;

    /// Link Status Register
    pub links:                          Volatile<u32>,          // 0x42A4;

    /// Auto-Negotiation Control 2 Register    
    pub autoc2:                         Volatile<u32>,          // 0x42A8;
    _padding20:                         [u8; 1620],             // 0x42AC - 0x48FF

    /// DCB Transmit Descriptor Plane Control and Status
    pub rttdcs:                         Volatile<u32>,          // 0x4900;
    _padding21:                         [u8; 380],              // 0x4904 - 0x4A7F

    /// DMA Tx Control
    pub dmatxctl:                       Volatile<u32>,          // 0x4A80;
    _padding22:                         [u8; 4],                // 0x4A84 - 0x4A87
    
    /// DMA Tx TCP Flags Control Low
    pub dtxtcpflgl:                     Volatile<u32>,          // 0x4A88;
    
    /// DMA Tx TCP Flags Control High
    pub dtxtcpflgh:                     Volatile<u32>,          // 0x4A8C;
    _padding23:                         [u8; 1392],             // 0x4A90 - 0x4FFF

    /// Receive Checksum Control
    pub rxcsum:                         Volatile<u32>,          // 0x5000
    _padding24:                         [u8; 124],              // 0x5004 - 0x507F

    /// Filter Control Register
    pub fctrl:                          Volatile<u32>,          // 0x5080;
    _padding25:                         [u8; 164],              // 0x5084 - 0x5127

    /// EType Queue Filter
    pub etqf:                           RegisterArray8,         // 0x5128 - 0x5147;
    _padding26:                         [u8; 3768],             // 0x5148 - 0x5FFF

    /// Set of registers for 128 transmit descriptor queues
    pub tx_regs:                        RegisterArrayTx,        // 0x6000 - 0x7FFF
    _padding27:                         [u8; 8704],             // 0x8000 - 0xA1FF

    /// Receive Address Low
    pub ral:                            Volatile<u32>,          // 0xA200;
    
    /// Receive Address High
    pub rah:                            Volatile<u32>,          // 0xA204;
    _padding28:                         [u8; 11768],            // 0xA208 - 0xCFFF

    /// Second set of Rx Registers for 64 Rx Queues
    pub rx_regs2:                       RegisterArrayRx,        // 0xD000 - 0xDFFF, for 64 queues

    /// Source Address Queue Filter
    pub saqf:                           RegisterArray128,       // 0xE000 - 0xE1FF
    
    /// Destination Address Queue Filter
    pub daqf:                           RegisterArray128,       // 0xE200 - 0xE3FF
    
    /// Source Destination Port Queue Filter
    pub sdpqf:                          RegisterArray128,       // 0xE400 - 0xE5FF
    
    /// Five Tuple Queue Filter
    pub ftqf:                           RegisterArray128,       // 0xE600 - 0xE7FF
    
    /// L3 L4 Tuples Immediate Interrupt Rx 
    pub l34timir:                       RegisterArray128,       // 0xE800 - 0xE9FF

    _padding29:                         [u8; 256],              // 0xEA00 - 0xEAFF

    /// Redirection Table
    pub reta:                           RegisterArray32,        // 0xEB00 - 0xEB7F

    /// RSS Random Key Register
    pub rssrk:                          RegisterArray10,        // 0xEB80 - 0xEBA7
    _padding30:                         [u8; 88],               // 0xEBA8 - 0xEBFF

    /// EType Queue Select
    pub etqs:                           RegisterArray8,         // 0xEC00 - 0xEC1F;
    _padding31:                         [u8; 96],               // 0xEC20 - 0xEC7F

    /// Multiple Receive Queues Command Register
    pub mrqc:                           Volatile<u32>,          // 0xEC80;
    _padding32:                         [u8; 5008],             // 0xEC84 - 0x10013

    /// EEPROM Read Register
    pub eerd:                           Volatile<u32>,          // 0x10014;
    _padding33:                         [u8; 296],              // 0x10018 - 0x1013F

    /// Software Semaphore Register
    pub swsm:                           Volatile<u32>,          // 0x10140
    _padding34:                         [u8; 28],               // 0x10144 - 0x1015F

    /// Software Firmware Synchronization
    pub sw_fw_sync:                     Volatile<u32>,          // 0x10160 
    _padding35:                         [u8; 3852],             // 0x10164 - 0x1106F

    /// DCA Requester ID Information Register
    pub dca_id:                         ReadOnly<u32>,          // 0x11070

    /// DCA Control Register
    pub dca_ctrl:                       Volatile<u32>,          // 0x11074
    _padding36:                         [u8; 61320],            // 0x11078 - 0x1FFFF
} //128 KB


/// Set of 4 32-bit registers
#[repr(C)]
pub struct RegisterArray4 {
    pub reg:                            [Volatile<u32>;4],
}

/// Set of 8 32-bit registers
#[repr(C)]
pub struct RegisterArray8 {
    pub reg:                            [Volatile<u32>;8],
}

/// Set of 10 32-bit registers
#[repr(C)]
pub struct RegisterArray10 {
    pub reg:                            [Volatile<u32>;10],
}

/// Set of 24 32-bit registers
#[repr(C)]
pub struct RegisterArray24 {
    pub reg:                           [Volatile<u32>;24],
}

/// Set of 32 32-bit registers
#[repr(C)]
pub struct RegisterArray32 {
    pub reg:                            [Volatile<u32>;32],
}

/// Set of 64 32-bit registers
#[repr(C)]
pub struct RegisterArray64 {
    pub reg:                            [Volatile<u32>;64],
}

/// Set of 104 32-bit registers
#[repr(C)]
pub struct RegisterArray104 {
    pub reg:                           [Volatile<u32>;104],
}

/// Set of 128 32-bit registers
#[repr(C)]
pub struct RegisterArray128 {
    pub reg:                           [Volatile<u32>;128],
}

/// Set of registers associated with one transmit descriptor queue
#[repr(C)]
pub struct RegistersTx {
    /// Transmit Descriptor Base Address Low
    pub tdbal:                          Volatile<u32>,          // 0x6000

    /// Transmit Descriptor Base Address High
    pub tdbah:                          Volatile<u32>,          // 0x6004
    
    /// Transmit Descriptor Length    
    pub tdlen:                          Volatile<u32>,          // 0x6008

    /// Tx DCA Control Register
    pub dca_txctrl:                     Volatile<u32>,          // 0x600C

    /// Transmit Descriptor Head
    pub tdh:                            Volatile<u32>,          // 0x6010
    _padding0:                          [u8; 4],                // 0x6014 - 0x6017

    /// Transmit Descriptor Tail
    pub tdt:                            Volatile<u32>,          // 0x6018
    _padding1:                          [u8; 12],               // 0x601C - 0x6027

    /// Transmit Descriptor Control
    pub txdctl:                         Volatile<u32>,          // 0x6028
    _padding2:                          [u8; 12],               // 0x602C - 0x6037

    /// Transmit Descriptor Completion Write Back Address Low
    pub tdwbal:                         Volatile<u32>,          // 0x6038

    /// Transmit Descriptor Completion Write Back Address High
    pub tdwbah:                         Volatile<u32>,          // 0x603C
} // 64B

/// Set of registers for 128 transmit descriptor queues
#[repr(C)]
pub struct RegisterArrayTx {
    pub tx_queue:                       [RegistersTx; 128],
} // 8KiB

/// Set of registers associated with one receive descriptor queue
#[repr(C)]
pub struct RegistersRx {
    /// Receive Descriptor Base Address Low
    pub rdbal:                          Volatile<u32>,          // 0x1000

    /// Recive Descriptor Base Address High
    pub rdbah:                          Volatile<u32>,          // 0x1004

    /// Recive Descriptor Length
    pub rdlen:                          Volatile<u32>,          // 0x1008

    /// Rx DCA Control Register
    pub dca_rxctrl:                     Volatile<u32>,          // 0x100C

    /// Recive Descriptor Head
    pub rdh:                            Volatile<u32>,          // 0x1010

    /// Split Receive Control Registers
    pub srrctl:                         Volatile<u32>,          // 0x1014 //specify descriptor type

    /// Receive Descriptor Tail
    pub rdt:                            Volatile<u32>,          // 0x1018
    _padding1:                          [u8;12],                // 0x101C - 0x1027

    /// Receive Descriptor Control
    pub rxdctl:                         Volatile<u32>,          // 0x1028
    _padding2:                          [u8;20],                // 0x102C - 0x103F                                            
} // 64B

/// Set of registers for 64 receive descriptor queues
#[repr(C)]
pub struct RegisterArrayRx {
    pub rx_queue:                       [RegistersRx; 64],
} // 4KiB

/// Offset where the RDT register starts for the first 64 queues
pub const RDT_1:                        usize = 0x1018;
/// Offset where the RDT register starts for the second set of 64 queues
pub const RDT_2:                        usize = 0xD018;
/// Number of bytes between consecutive RDT registers
pub const RDT_DIST:                     usize = 0x40;
/// Offset where the RDT register starts for the first 64 queues
pub const TDT:                          usize = 0x6018;
/// Number of bytes between consecutive TDT registers
pub const TDT_DIST:                     usize = 0x40;

// Link set up commands
pub const AUTOC_LMS_CLEAR:              u32 = 0x0000_E000; 
pub const AUTOC_LMS_1_GB:               u32 = 0x0000_E000;
pub const AUTOC_LMS_10_GBE_P:           u32 = 1 << 13;
pub const AUTOC_LMS_10_GBE_S:           u32 = 3 << 13;
pub const AUTOC_FLU:                    u32 = 1;
pub const AUTOC_LMS:                    u32 = 6<<13; //KX/KX4//KR
pub const AUTOC_RESTART_AN:             u32 = 1<<12;
pub const AUTOC_1G_PMA_PMD:             u32 = 0x0000_0200; //clear bit 9
pub const AUTOC_10G_PMA_PMD_P:          u32 = 1 << 7; 
pub const AUTOC2_10G_PMA_PMD_S_CLEAR:   u32 = 0x0003_0000; //clear bits 16 and 17 
pub const AUTOC2_10G_PMA_PMD_S_SFI:     u32 = 1 << 17;

// CTRL commands
pub const CTRL_LRST:                    u32 = (1<<3); 
pub const CTRL_RST:                     u32 = (1<<26);

// semaphore commands
pub const SWSM_SMBI:                    u32 = 1 << 0;
pub const SWSM_SWESMBI:                 u32 = 1 << 1;
pub const SW_FW_SYNC_SMBITS_MASK:       u32 = 0x3FF;
pub const SW_FW_SYNC_SMBITS_SW:         u32 = 0x1F;
pub const SW_FW_SYNC_SMBITS_FW:         u32 = 0x3E0;
pub const SW_FW_SYNC_SW_MAC:            u32 = 1 << 3;
pub const SW_FW_SYNC_FW_MAC:            u32 = 1 << 8;

// RCTL commands
pub const BSIZEPACKET_8K:               u32 = 8;
pub const BSIZEHEADER_256B:             u32 = 4;
pub const DESCTYPE_LEG:                 u32 = 0;
pub const DESCTYPE_ADV_1BUFFER:         u32 = 1;
pub const DESCTYPE_ADV_HS:              u32 = 2;
pub const RX_Q_ENABLE:                  bool = true;
pub const STORE_BAD_PACKETS:            u32 = 1 << 1;
pub const MULTICAST_PROMISCUOUS_ENABLE: u32 = 1 << 8;
pub const UNICAST_PROMISCUOUS_ENABLE:   u32 = 1 << 9;
pub const BROADCAST_ACCEPT_MODE:        u32 = 1 << 20;
pub const RECEIVE_ENABLE:               u32 = 1;

// RSS commands
pub const RXCSUM_PCSD:                  u32 = 1 << 13; 
pub const MRQC_MRQE_RSS:                u32 = 1; // set bits 0..3 in MRQC
pub const MRQC_TCPIPV4:                 u32 = 1 << 16; 
pub const MRQC_IPV4:                    u32 = 1 << 17; 
pub const MRQC_IPV6:                    u32 = 1 << 20;
pub const MRQC_TCPIPV6:                 u32 = 1 << 21;  
pub const MRQC_UDPIPV4:                 u32 = 1 << 22; 
pub const MRQC_UDPIPV6:                 u32 = 1 << 23;  
pub const RETA_ENTRY_0_OFFSET:          u32 = 0;
pub const RETA_ENTRY_1_OFFSET:          u32 = 8;
pub const RETA_ENTRY_2_OFFSET:          u32 = 16;
pub const RETA_ENTRY_3_OFFSET:          u32 = 24;

// DCA commands
pub const RX_DESC_DCA_ENABLE:           u32 = 1 << 5;
pub const RX_HEADER_DCA_ENABLE:         u32 = 1 << 6;
pub const RX_PAYLOAD_DCA_ENABLE:        u32 = 1 << 7;
pub const RX_DESC_R_RELAX_ORDER_EN:     u32 = 1 << 9;
pub const RX_DATA_W_RELAX_ORDER_EN:     u32 = 1 << 13;
pub const RX_SP_HEAD_RELAX_ORDER_EN:    u32 = 1 << 15;
pub const DCA_CPUID_SHIFT:              u32 = 24;
pub const DCA_ENABLE:                   u32 = 0;
pub const DCA_MODE_1:                   u32 = 0 << 1;  
pub const DCA_MODE_2:                   u32 = 1 << 1;

// 5-tuple Queue Filter commands
pub const SPDQF_SOURCE_SHIFT:           u32 = 0;
pub const SPDQF_DEST_SHIFT:             u32 = 16;
pub const FTQF_PROTOCOL:                u32 = 3;
pub const FTQF_PROTOCOL_TCP:            u32 = 0;
pub const FTQF_PROTOCOL_UDP:            u32 = 1;
pub const FTQF_PROTOCOL_SCTP:           u32 = 2;
pub const FTQF_PRIORITY:                u32 = 7;
pub const FTQF_PRIORITY_SHIFT:          u32 = 2;
pub const FTQF_SOURCE_ADDRESS_MASK:     u32 = 1 << 25;
pub const FTQF_DEST_ADDRESS_MASK:       u32 = 1 << 26;
pub const FTQF_SOURCE_PORT_MASK:        u32 = 1 << 27;
pub const FTQF_DEST_PORT_MASK:          u32 = 1 << 28;
pub const FTQF_PROTOCOL_MASK:           u32 = 1 << 29;
pub const FTQF_POOL_MASK:               u32 = 1 << 30;
pub const FTQF_Q_ENABLE:                u32 = 1 << 31;
pub const L34TIMIR_BYPASS_SIZE_CHECK:   u32 = 1 << 12;
pub const L34TIMIR_RESERVED:            u32 = 0x40 << 13;
pub const L34TIMIR_LLI_ENABLE:          u32 = 1 << 20;
pub const L34TIMIR_RX_Q_SHIFT:          u32 = 21;

 
// Buffer Sizes
pub const RCTL_BSIZE_256:               u32 = (3 << 16);
pub const RCTL_BSIZE_512:               u32 = (2 << 16);
pub const RCTL_BSIZE_1024:              u32 = (1 << 16);
pub const RCTL_BSIZE_2048:              u32 = (0 << 16);
pub const RCTL_BSIZE_4096:              u32 = ((3 << 16) | (1 << 25));
pub const RCTL_BSIZE_8192:              u32 = ((2 << 16) | (1 << 25));
pub const RCTL_BSIZE_16384:             u32 = ((1 << 16) | (1 << 25));
  
 
// TCTL commands
pub const TX_Q_ENABLE:                  bool = true;
pub const TE:                           u32  = 1;           //Transmit Enable
pub const TCTL_EN:                      u32 = (1 << 1);     // Transmit Enable
pub const TCTL_PSP:                     u32 = (1 << 3);     // Pad Short Packets
pub const TCTL_CT_SHIFT:                u32 = 4;            // Collision Threshold
pub const TCTL_COLD_SHIFT:              u32 = 12;           // Collision Distance
pub const TCTL_SWXOFF:                  u32 = (1 << 22);    // Software XOFF Transmission
pub const TCTL_RTLC:                    u32 = (1 << 24);    // Re-transmit on Late Collision
pub const TSTA_DD:                      u32 = (1 << 0);     // Descriptor Done
pub const TSTA_EC:                      u32 = (1 << 1);     // Excess Collisions
pub const TSTA_LC:                      u32 = (1 << 2);     // Late Collision
pub const LSTA_TU:                      u32 = (1 << 3);     // Transmit Underrun

// Interrupt Register Commands 
pub const DISABLE_INTERRUPTS:           u32 = 0x7FFFFFFF; 
/// MSI-X Mode
pub const GPIE_MULTIPLE_MSIX:           u32 = 1 << 4;
/// EICS Immediate Interrupt Enable
pub const GPIE_EIMEN:                   u32 = 1 << 6;
/// Should be set in MSIX mode and cleared in legacy/msi mode
pub const GPIE_PBA_SUPPORT:             u32 = 1 << 31;
/// Each bit enables auto clear of the corresponding RTxQ bit in the EICR register following interrupt assertion
pub const EIAC_RTXQ_AUTO_CLEAR:         u32 = 0xFFFF;
/// Bit position where the throttling interval is written
pub const EITR_ITR_INTERVAL_SHIFT:      u32 = 3;
/// Enables the corresponding interrupt in the EICR register by setting the bit
pub const EIMS_INTERRUPT_ENABLE:        u32 = 1;

/// The number of msi-x vectors this device can have. 
/// It can be set from PCI space, but we took the value from the data sheet.
pub const IXGBE_MAX_MSIX_VECTORS:     usize = 64;

/// Table that contains msi-x vector entries. 
/// It is mapped to a physical memory region specified by the BAR from the PCI space.
#[repr(C)]
pub struct MsixVectorTable {
    pub msi_vector:     [MsixVectorEntry; IXGBE_MAX_MSIX_VECTORS],
}

/// A single Message Signaled Interrupt entry.
/// It contains the interrupt number for this vector and the core this interrupt is redirected to.
#[repr(C)]
pub struct MsixVectorEntry {
    /// The lower portion of the address for the memory write transaction.
    /// This part contains the apic id which the interrupt will be redirected to.
    pub msg_lower_addr:         Volatile<u32>,
    /// The upper portion of the address for the memory write transaction.
    pub msg_upper_addr:         Volatile<u32>,
    /// The data portion of the msi vector which contains the interrupt number.
    pub msg_data:               Volatile<u32>,
    /// The control portion which contains the interrupt mask bit.
    pub vector_control:         Volatile<u32>,
}

/// A constant which indicates the region that is reserved for interrupt messages
pub const MSIX_INTERRUPT_REGION:    u32 = 0xFEE << 20;
/// The location in the lower address register where the destination core id is written
pub const MSIX_DEST_ID_SHIFT:       u32 = 12;
/// The bits in the lower address register that need to be cleared and set
pub const MSIX_ADDRESS_BITS:        u32 = 0xFFFF_FFF0;
/// Clear the vector control field to unmask the interrupt
pub const MSIX_UNMASK_INT:          u32 = 0;