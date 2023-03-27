//! This file contains the structs that are used to access device registers and contains configuration values to write to registers.
//! 
//! The registers are divided into multiple structs because we need to separate out the 
//! receive and transmit queue registers and store them separately for virtualization. 
//! 
//! The 7 structs which cover the registers of the entire memory-mapped region are:
//! * `IntelIxgbeRegisters1`
//! * `IntelIxgbeRxRegisters1`
//! * `IntelIxgbeRegisters2`
//! * `IntelIxgbeTxRegisters`
//! * `IntelIxgbeMacRegisters`
//! * `IntelIxgbeRxRegisters2`
//! * `IntelIxgbeRegisters3`

use volatile::{Volatile, ReadOnly, WriteOnly};
use zerocopy::FromBytes;

/// The layout in memory of the first set of general registers of the 82599 device.
#[derive(FromBytes)]
#[repr(C)]
pub struct IntelIxgbeRegisters1 {
    /// Device Control Register
    pub ctrl:                           Volatile<u32>,          // 0x0
    _padding0:                          [u8; 4],                // 0x4 - 0x7
    
    /// Device Status Register
    pub status:                         ReadOnly<u32>,          // 0x8
    _padding1:                          [u8; 12],               // 0xC - 0x17

    /// Extended Device Control Register
    pub ctrl_ext:                       Volatile<u32>,          // 0x18
    _padding2:                          [u8; 12],               // 0x1C - 0x27

    /// I2C Control
    pub i2cctl:                         Volatile<u32>,          // 0x28
    _padding3:                          [u8; 2004],             // 0x2C - 0x7FF

    /// Extended Interrupt Cause Register
    pub eicr:                           Volatile<u32>,          // 0x800
    _padding4:                          [u8; 4],                // 0x804 - 0x807

    /// Extended Interrupt Cause Set Register
    pub eics:                           WriteOnly<u32>,         // 0x808
    _padding5:                          [u8; 4],                // 0x80C - 0x80F

    /// Extended Interrupt Auto Clear Register
    pub eiac:                           Volatile<u32>,          // 0x810; 
    _padding6:                          [u8; 12],               // 0x814 - 0x81F 
    
    /// Extended Interrupt Throttle Registers
    pub eitr:                           [Volatile<u32>; 24],    // 0x820 - 0x87F; 

    /// Extended Interrupt Mask Set/ Read Register
    pub eims:                           Volatile<u32>,          // 0x880; 
    _padding7:                          [u8; 4],                // 0x884 - 0x887

    /// Extended Interrupt Mask Clear Register
    pub eimc:                           WriteOnly<u32>,         // 0x888; 
    _padding8:                          [u8; 4],                // 0x88C - 0x88F     

    /// Extended Interrupt Auto Mask Enable Register
    pub eiam:                           Volatile<u32>,          // 0x890; 
    _padding9:                          [u8; 4],                // 0x894 - 0x897

    /// General Purpose Interrupt Enable
    pub gpie:                           Volatile<u32>,          // 0x898; 
    _padding10:                         [u8; 100],              // 0x89C - 0x8FF

    /// Interrupt Vector Allocation Registers
    pub ivar:                           [Volatile<u32>; 64],    // 0x900 - 0x9FF  
    _padding11:                         [u8; 1536],             // 0xA00 - 0xFFF

} // 1 4KiB page

const _: () = assert!(core::mem::size_of::<IntelIxgbeRegisters1>() == 4096);

/// The layout in memory of the first set of receive queue registers of the 82599 device.
#[derive(FromBytes)]
#[repr(C)]
pub struct IntelIxgbeRxRegisters1 {
    /// First set of Rx Registers for 64 Rx Queues
    pub rx_regs1:                       [RegistersRx; 64],      // 0x1000 - 0x1FFF

} // 1 4KiB page

const _: () = assert!(core::mem::size_of::<IntelIxgbeRxRegisters1>() == 4096);

/// The layout in memory of the second set of general registers of the 82599 device.
#[derive(FromBytes)]
#[repr(C)]
pub struct IntelIxgbeRegisters2 {
    _padding1:                          [u8; 3840],             // 0x2000 - 0x2EFF
    
    /// Receive DMA Control Register
    pub rdrxctl:                        Volatile<u32>,          // 0x2F00;
    _padding2:                          [u8; 252],              // 0x2F04 - 0x2FFF

    /// Receive Control Register
    pub rxctrl:                         Volatile<u32>,          // 0x3000;
    _padding3:                          [u8; 508],              // 0x3004 - 0x31FF

    /// Flow Control Transmit Timer Value
    pub fcttv:                          [Volatile<u32>;4],      // 0x3200 - 0x320F
    _padding4:                          [u8; 16],               // 0x3210 - 0x321F

    /// Flow Control Receive Threshold Low
    pub fcrtl:                          [Volatile<u32>;8],      // 0x3220 - 0x323F
    _padding5:                          [u8; 32],               // 0x3240 - 0x325F 

    /// Flow Control Receive Threshold High
    pub fcrth:                          [Volatile<u32>;8],      // 0x3260 - 0x327F
    _padding6:                          [u8; 32],               // 0x3280 - 0x329F

    /// Flow Control Refresh Threshold Value
    pub fcrtv:                          Volatile<u32>,          // 0x32A0;
    _padding7:                          [u8; 2396],             // 0x32A4 - 0x3CFF

    ///Receive Packet Buffer Size
    pub rxpbsize:                       [Volatile<u32>;8],      // 0x3C00   
    _padding8:                          [u8; 224],              // 0x3C20 - 0x3CFF        

    /// Flow Control Configuration
    pub fccfg:                          Volatile<u32>,          // 0x3D00;
    _padding9:                          [u8; 880],              // 0x3D04 - 0x4073

    /// Good Packets Received Count
    pub gprc:                           Volatile<u32>,          // 0x4074
    _padding10:                         [u8; 8],                // 0x4078 - 0x407F

    /// Good Packets Transmitted Count
    pub gptc:                           Volatile<u32>,          // 0x4080
    _padding11:                         [u8; 4],                // 0x4084 - 0x4087 

    /// Good Octets Received Count Low
    pub gorcl:                          Volatile<u32>,          // 0x4088

    /// Good Octets Received Count High
    pub gorch:                          Volatile<u32>,          // 0x408C
    
    /// Good Octets Transmitted Count Low
    pub gotcl:                          Volatile<u32>,          // 0x4090

    /// Good Octets Transmitted Count High
    pub gotch:                          Volatile<u32>,          // 0x4094
    _padding12:                         [u8; 424],              // 0x4098 - 0x423F

    /// MAC Core Control 0 Register 
    pub hlreg0:                         Volatile<u32>,          // 0x4240;
    _padding13:                         [u8; 92],               // 0x4244 - 0x429F

    /// Auto-Negotiation Control Register
    pub autoc:                          Volatile<u32>,          // 0x42A0;

    /// Link Status Register
    pub links:                          Volatile<u32>,          // 0x42A4;

    /// Auto-Negotiation Control 2 Register    
    pub autoc2:                         Volatile<u32>,          // 0x42A8;
    _padding14:                         [u8; 120],              // 0x42AC - 0x4323

    /// Link Status Register 2
    pub links2:                         Volatile<u32>,          // 0x4324
    _padding15:                         [u8; 1496],             // 0x4328 - 0x48FF

    /// DCB Transmit Descriptor Plane Control and Status
    pub rttdcs:                         Volatile<u32>,          // 0x4900;
    _padding16:                         [u8; 380],              // 0x4904 - 0x4A7F

    /// DMA Tx Control
    pub dmatxctl:                       Volatile<u32>,          // 0x4A80;
    _padding17:                         [u8; 4],                // 0x4A84 - 0x4A87
    
    /// DMA Tx TCP Flags Control Low
    pub dtxtcpflgl:                     Volatile<u32>,          // 0x4A88;
    
    /// DMA Tx TCP Flags Control High
    pub dtxtcpflgh:                     Volatile<u32>,          // 0x4A8C;
    _padding18:                         [u8; 1392],             // 0x4A90 - 0x4FFF

    /// Receive Checksum Control
    pub rxcsum:                         Volatile<u32>,          // 0x5000
    _padding19:                         [u8; 124],              // 0x5004 - 0x507F

    /// Filter Control Register
    pub fctrl:                          Volatile<u32>,          // 0x5080;
    _padding20:                         [u8; 164],              // 0x5084 - 0x5127

    /// EType Queue Filter
    pub etqf:                           [Volatile<u32>;8],      // 0x5128 - 0x5147;
    _padding21:                         [u8; 3768],             // 0x5148 - 0x5FFF
} // 4 4KiB page

const _: () = assert!(core::mem::size_of::<IntelIxgbeRegisters2>() == 4 * 4096);

/// The layout in memory of the transmit queue registers of the 82599 device.
#[derive(FromBytes)]
#[repr(C)]
pub struct IntelIxgbeTxRegisters {
    /// Set of registers for 128 transmit descriptor queues
    pub tx_regs:                        [RegistersTx; 128],     // 0x6000 - 0x7FFF
} // 2 4KiB page

const _: () = assert!(core::mem::size_of::<IntelIxgbeTxRegisters>() == 2 * 4096);

/// The layout in memory of the set of registers containing the MAC address of the 82599 device.
#[derive(FromBytes)]
#[repr(C)]
pub struct IntelIxgbeMacRegisters {
    _padding1:                          [u8; 256],              // 0x8000 - 0x80FF
    /// DMA Tx TCP Max Allow Size Requests
    pub dtxmxszrq:                      Volatile<u32>,          // 0X8100
    _padding2:                          [u8; 8444],             // 0x8104 - 0xA1FF
    
    /// Receive Address Low
    pub ral:                            Volatile<u32>,          // 0xA200;
    
    /// Receive Address High
    pub rah:                            Volatile<u32>,          // 0xA204;
    _padding3:                          [u8; 10744],            // 0xA208 - 0xCBFF

    /// Transmit Packet Buffer Size
    pub txpbsize:                       [Volatile<u32>;8],      // 0xCC00
    _padding4:                          [u8; 992],              // 0xCC20 - 0xCFFF
} // 5 4KiB page

const _: () = assert!(core::mem::size_of::<IntelIxgbeMacRegisters>() == 5 * 4096);

/// The layout in memory of the second set of receive queue registers of the 82599 device.
#[derive(FromBytes)]
#[repr(C)]
pub struct IntelIxgbeRxRegisters2 {
    /// Second set of Rx Registers for 64 Rx Queues
    pub rx_regs2:                       [RegistersRx; 64],      // 0xD000 - 0xDFFF, for 64 queues
} // 1 4KiB page

const _: () = assert!(core::mem::size_of::<IntelIxgbeRxRegisters2>() == 4096);

/// The layout in memory of the third set of general registers of the 82599 device.
#[derive(FromBytes)]
#[repr(C)]
pub struct IntelIxgbeRegisters3 {
    /// Source Address Queue Filter
    pub saqf:                           [Volatile<u32>;128],    // 0xE000 - 0xE1FF
    
    /// Destination Address Queue Filter
    pub daqf:                           [Volatile<u32>;128],    // 0xE200 - 0xE3FF
    
    /// Source Destination Port Queue Filter
    pub sdpqf:                          [Volatile<u32>;128],    // 0xE400 - 0xE5FF
    
    /// Five Tuple Queue Filter
    pub ftqf:                           [Volatile<u32>;128],    // 0xE600 - 0xE7FF
    
    /// L3 L4 Tuples Immediate Interrupt Rx 
    pub l34timir:                       [Volatile<u32>;128],    // 0xE800 - 0xE9FF

    _padding1:                          [u8; 256],              // 0xEA00 - 0xEAFF

    /// Redirection Table
    pub reta:                           [Volatile<u32>;32],     // 0xEB00 - 0xEB7F

    /// RSS Random Key Register
    pub rssrk:                          [Volatile<u32>;10],     // 0xEB80 - 0xEBA7
    _padding2:                          [u8; 88],               // 0xEBA8 - 0xEBFF

    /// EType Queue Select
    pub etqs:                           [Volatile<u32>;8],      // 0xEC00 - 0xEC1F;
    _padding3:                          [u8; 96],               // 0xEC20 - 0xEC7F

    /// Multiple Receive Queues Command Register
    pub mrqc:                           Volatile<u32>,          // 0xEC80;
    _padding4:                          [u8; 5004],             // 0xEC84 - 0x1000F

    /// EEPROM/ Flash Control Register
    pub eec:                            Volatile<u32>,          // 0x10010

    /// EEPROM Read Register
    pub eerd:                           Volatile<u32>,          // 0x10014;
    _padding5:                          [u8; 296],              // 0x10018 - 0x1013F

    /// Software Semaphore Register
    pub swsm:                           Volatile<u32>,          // 0x10140
    _padding6:                          [u8; 28],               // 0x10144 - 0x1015F

    /// Software Firmware Synchronization
    pub sw_fw_sync:                     Volatile<u32>,          // 0x10160 
    _padding7:                          [u8; 3852],             // 0x10164 - 0x1106F

    /// DCA Requester ID Information Register
    pub dca_id:                         ReadOnly<u32>,          // 0x11070

    /// DCA Control Register
    pub dca_ctrl:                       Volatile<u32>,          // 0x11074
    _padding8:                          [u8; 61320],            // 0x11078 - 0x1FFFF

} // 18 4KiB page (total NIC mem = 128 KB)

const _: () = assert!(core::mem::size_of::<IntelIxgbeRegisters3>() == 18 * 4096);

// check that the sum of all the register structs is equal to the memory of the ixgbe device (128 KiB).
const _: () = assert!(
    core::mem::size_of::<IntelIxgbeRegisters1>()
    + core::mem::size_of::<IntelIxgbeRxRegisters1>()
    + core::mem::size_of::<IntelIxgbeRegisters2>()
    + core::mem::size_of::<IntelIxgbeTxRegisters>()
    + core::mem::size_of::<IntelIxgbeMacRegisters>()
    + core::mem::size_of::<IntelIxgbeRxRegisters2>()
    + core::mem::size_of::<IntelIxgbeRegisters3>()
    == 0x20000
);

/// Set of registers associated with one transmit descriptor queue.
#[derive(FromBytes)]
#[repr(C)]
pub struct RegistersTx {
    /// Transmit Descriptor Base Address Low
    pub tdbal:                          Volatile<u32>,        // 0x6000

    /// Transmit Descriptor Base Address High
    pub tdbah:                          Volatile<u32>,        // 0x6004
    
    /// Transmit Descriptor Length    
    pub tdlen:                          Volatile<u32>,        // 0x6008

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

const _: () = assert!(core::mem::size_of::<RegistersTx>() == 64);

/// Set of registers associated with one receive descriptor queue.
#[derive(FromBytes)]
#[repr(C)]
pub struct RegistersRx {
    /// Receive Descriptor Base Address Low
    pub rdbal:                          Volatile<u32>,        // 0x1000

    /// Recive Descriptor Base Address High
    pub rdbah:                          Volatile<u32>,        // 0x1004

    /// Recive Descriptor Length
    pub rdlen:                          Volatile<u32>,        // 0x1008

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

const _: () = assert!(core::mem::size_of::<RegistersRx>() == 64);

/// Offset where the RDT register starts for the first 64 rx queues
pub const RDT_1:                        usize = 0x1018;
/// Offset where the RDT register starts for the second set of 64 rx queues
pub const RDT_2:                        usize = 0xD018;
/// Number of bytes between consecutive RDT registers
pub const RDT_DIST:                     usize = 0x40;
/// Offset where the TDT register starts for the first 64 queues
pub const TDT:                          usize = 0x6018;
/// Number of bytes between consecutive TDT registers
pub const TDT_DIST:                     usize = 0x40;

// Link set up commands
pub const AUTOC_LMS_CLEAR:              u32 = 0x0000_E000; 
pub const AUTOC_LMS_1_GB:               u32 = 0x0000_E000;
pub const AUTOC_LMS_10_GBE_P:           u32 = 1 << 13;
pub const AUTOC_LMS_10_GBE_S:           u32 = 3 << 13;
pub const AUTOC_LMS_KX_KX4_AUTONEG:     u32 = 6<<13; //KX/KX4//KR
pub const AUTOC_FLU:                    u32 = 1;
pub const AUTOC_RESTART_AN:             u32 = 1<<12;
pub const AUTOC_1G_PMA_PMD:             u32 = 0x0000_0200; //clear bit 9
pub const AUTOC_10G_PMA_PMD_CLEAR:      u32 = 0x0000_0180; 
pub const AUTOC_10G_PMA_PMD_XAUI:       u32 = 0 << 7; 
pub const AUTOC2_10G_PMA_PMD_S_CLEAR:   u32 = 0x0003_0000; //clear bits 16 and 17 
pub const AUTOC2_10G_PMA_PMD_S_SFI:     u32 = 1 << 17;

// CTRL commands
pub const CTRL_LRST:                    u32 = 1<<3; 
pub const CTRL_RST:                     u32 = 1<<26;

// semaphore commands
pub const SWSM_SMBI:                    u32 = 1 << 0;
pub const SWSM_SWESMBI:                 u32 = 1 << 1;
pub const SW_FW_SYNC_SMBITS_MASK:       u32 = 0x3FF;
pub const SW_FW_SYNC_SMBITS_SW:         u32 = 0x1F;
pub const SW_FW_SYNC_SMBITS_FW:         u32 = 0x3E0;
pub const SW_FW_SYNC_SW_MAC:            u32 = 1 << 3;
pub const SW_FW_SYNC_FW_MAC:            u32 = 1 << 8;

// EEPROM Commands
/// Bit which indicates that auto-read by hardware from EEPROM is done
pub const EEC_AUTO_RD:                  u32 = 9;

// Link Commands
pub const LINKS_SPEED_MASK:             u32 = 0x3 << 28;

// MAC Control Commands
/// Tx CRC Enable by HW (bit 0)
pub const HLREG0_TXCRCEN:               u32 = 1;
/// Tx Pad Frame Enable (bit 10)
pub const HLREG0_TXPADEN:               u32 = 1 << 10;
/// Enable CRC strip by HW
pub const HLREG0_CRC_STRIP:             u32 = 1 << 1;
/// Enable CRC strip by HW
pub const RDRXCTL_CRC_STRIP:            u32 = 1;
/// These 5 bits have to be cleared by software
pub const RDRXCTL_RSCFRSTSIZE:          u32 = 0x1F << 17;

/// DCB Arbiters Disable
pub const RTTDCS_ARBDIS:                u32 = 1 << 6;

/// For DCB and VT disabled, set TXPBSIZE.SIZE to 160KB
pub const TXPBSIZE_160KB:                u32 = 0xA0 << 10;
/// For DCB and VT disabled, set RXPBSIZE.SIZE to 512KB
pub const RXPBSIZE_512KB:                u32 = 0x200 << 10;

// RCTL commands
pub const BSIZEPACKET_8K:               u32 = 8;
pub const BSIZEHEADER_256B:             u32 = 4;
pub const BSIZEHEADER_0B:               u32 = 0;
pub const DESCTYPE_LEG:                 u32 = 0;
pub const DESCTYPE_ADV_1BUFFER:         u32 = 1;
pub const DESCTYPE_ADV_HS:              u32 = 2;
pub const RX_Q_ENABLE:                  u32 = 1 << 25;
pub const STORE_BAD_PACKETS:            u32 = 1 << 1;
pub const MULTICAST_PROMISCUOUS_ENABLE: u32 = 1 << 8;
pub const UNICAST_PROMISCUOUS_ENABLE:   u32 = 1 << 9;
pub const BROADCAST_ACCEPT_MODE:        u32 = 1 << 10;
pub const RECEIVE_ENABLE:               u32 = 1;
pub const DROP_ENABLE:                  u32 = 1 << 28;
pub const DCA_RXCTRL_CLEAR_BIT_12:      u32 = 1 << 12;
pub const CTRL_EXT_NO_SNOOP_DIS:        u32 = 1 << 16;

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
pub const DCA_CTRL_ENABLE:              u32 = 0;
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
pub const RCTL_BSIZE_256:               u32 = 3 << 16;
pub const RCTL_BSIZE_512:               u32 = 2 << 16;
pub const RCTL_BSIZE_1024:              u32 = 1 << 16;
pub const RCTL_BSIZE_2048:              u32 = 0 << 16;
pub const RCTL_BSIZE_4096:              u32 = (3 << 16) | (1 << 25);
pub const RCTL_BSIZE_8192:              u32 = (2 << 16) | (1 << 25);
pub const RCTL_BSIZE_16384:             u32 = (1 << 16) | (1 << 25);
  
 
/// Enable a transmit queue
pub const TX_Q_ENABLE:                  u32 = 1 << 25;
/// Transmit Enable
pub const TE:                           u32  = 1;           
pub const DTXMXSZRQ_MAX_BYTES:          u32 = 0xFFF;

/// Tx descriptor pre-fetch threshold (value taken from DPDK)
pub const TXDCTL_PTHRESH:               u32 = 36; 
/// Tx descriptor host threshold (value taken from DPDK)
pub const TXDCTL_HTHRESH:               u32 = 8 << 8; 
/// Tx descriptor write-back threshold (value taken from DPDK)
pub const TXDCTL_WTHRESH:               u32 = 4 << 16; 

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
#[derive(FromBytes)]
#[repr(C)]
pub struct MsixVectorTable {
    pub msi_vector:     [MsixVectorEntry; IXGBE_MAX_MSIX_VECTORS],
}

/// A single Message Signaled Interrupt entry.
/// It contains the interrupt number for this vector and the core this interrupt is redirected to.
#[derive(FromBytes)]
#[repr(C)]
pub struct MsixVectorEntry {
    /// The lower portion of the address for the memory write transaction.
    /// This part contains the CPU ID which the interrupt will be redirected to.
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
