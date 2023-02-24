//! This file contains the structs that are used to access device registers and contains configuration values to write to registers.
//! 
//! The registers are divided into multiple structs because we need to separate out the
//! receive and transmit queue registers and store them separately in a per-queue struct.
//! Though the e1000 device only has 1 pair of receive and transmit queues, we still structure
//! the design this way to be able to use code shared by all network drivers.
//! 
//! The 4 structs which cover the registers of the entire memory-mapped region are:
//! * `E1000Registers`
//! * `E1000RxRegisters`
//! * `E1000TxRegisters`
//! * `E1000MacRegisters`


use volatile::{Volatile, ReadOnly};
use zerocopy::FromBytes;

/// The layout in memory of the first set of e1000 registers. 
#[derive(FromBytes)]
#[repr(C)]
pub struct E1000Registers {
    pub ctrl:                       Volatile<u32>,          // 0x0
    _padding0:                      [u8; 4],                // 0x4 - 0x7
    pub status:                     ReadOnly<u32>,          // 0x8
    _padding1:                      [u8; 180],              // 0xC - 0xBF,  180 bytes
    
    /// Interrupt control registers
    pub icr:                        ReadOnly<u32>,          // 0xC0   
    _padding2:                      [u8; 12],               // 0xC4 - 0xCF
    pub ims:                        Volatile<u32>,          // 0xD0
    _padding3:                      [u8; 44],               // 0xD4 - 0xFF 

    /// Receive control register
    pub rctl:                       Volatile<u32>,          // 0x100
    _padding4:                      [u8; 764],              // 0x104 - 0x3FF,  764 bytes
    
    /// Transmit control register
    pub tctl:                       Volatile<u32>,          // 0x400
    _padding5:                      [u8; 7164],             // 0x404 - 0x1FFF
    
} // 2 4KiB pages

const _: () = assert!(core::mem::size_of::<E1000Registers>() == 2 * 4096);

/// The layout in memory of e1000 receive registers. 
#[derive(FromBytes)]
#[repr(C)]
pub struct E1000RxRegisters {
    _padding6:                      [u8; 2048],             // 0x2000 - 0x27FF

    pub rx_regs:                    RegistersRx,            // 0x2800    
    _padding7:                      [u8; 2020],             // 0x281C - 0x2FFF
} // 1 4KiB page

const _: () = assert!(core::mem::size_of::<E1000RxRegisters>() == 4096);


/// The layout in memory of e1000 transmit registers. 
#[derive(FromBytes)]
#[repr(C)]
pub struct E1000TxRegisters {
    _padding8:                      [u8; 2048],             // 0x3000 - 0x37FF

    pub tx_regs:                    RegistersTx,            // 0x3800
    _padding9:                      [u8; 2020],             // 0x381C - 0x3FFF
} // 1 4KiB page

const _: () = assert!(core::mem::size_of::<E1000TxRegisters>() == 4096);


/// The layout in memory of e1000 MAC address registers. 
#[derive(FromBytes)]
#[repr(C)]
pub struct E1000MacRegisters {
    _padding10:                     [u8; 5120],             // 0x4000 - 0x53FF
    
    /// The lower (least significant) 32 bits of the NIC's MAC hardware address.
    pub ral:                        Volatile<u32>,          // 0x5400
    /// The higher (most significant) 32 bits of the NIC's MAC hardware address.
    pub rah:                        Volatile<u32>,          // 0x5404
    _padding11:                     [u8; 109560],           // 0x5408 - 0x1FFFF,  109560 bytes
    // End of all register structs should be at offset 0x20000 (128 KiB in total size).

} // 28 4KiB pages

const _: () = assert!(core::mem::size_of::<E1000MacRegisters>() == 28 * 4096);

// check that the sum of all the register structs is equal to the memory of the e1000 device (128 KiB).
const _: () = assert!(
    core::mem::size_of::<E1000Registers>()
    + core::mem::size_of::<E1000RxRegisters>()
    + core::mem::size_of::<E1000TxRegisters>()
    + core::mem::size_of::<E1000MacRegisters>()
    == 0x20000
);


/// Struct that holds registers related to one receive queue.
#[derive(FromBytes)]
#[repr(C)]
pub struct RegistersRx {
    /// The lower (least significant) 32 bits of the physical address of the array of receive descriptors.
    pub rdbal:                      Volatile<u32>,        // 0x2800
    /// The higher (most significant) 32 bits of the physical address of the array of receive descriptors.
    pub rdbah:                      Volatile<u32>,        // 0x2804
    /// The length in bytes of the array of receive descriptors.
    pub rdlen:                      Volatile<u32>,        // 0x2808
    _padding0:                      [u8; 4],                // 0x280C - 0x280F
    /// The receive descriptor head index, which points to the next available receive descriptor.
    pub rdh:                        Volatile<u32>,          // 0x2810
    _padding1:                      [u8; 4],                // 0x2814 - 0x2817
    /// The receive descriptor tail index, which points to the last available receive descriptor.
    pub rdt:                        Volatile<u32>,          // 0x2818
}


/// Struct that holds registers related to one transmit queue.
#[derive(FromBytes)]
#[repr(C)]
pub struct RegistersTx {
    /// The lower (least significant) 32 bits of the physical address of the array of transmit descriptors.
    pub tdbal:                      Volatile<u32>,        // 0x3800
    /// The higher (most significant) 32 bits of the physical address of the array of transmit descriptors.
    pub tdbah:                      Volatile<u32>,        // 0x3804
    /// The length in bytes of the array of transmit descriptors.
    pub tdlen:                      Volatile<u32>,        // 0x3808
    _padding0:                      [u8; 4],                // 0x380C - 0x380F
    /// The transmit descriptor head index, which points to the next available transmit descriptor.
    pub tdh:                        Volatile<u32>,          // 0x3810
    _padding1:                      [u8; 4],                // 0x3814 - 0x3817
    /// The transmit descriptor tail index, which points to the last available transmit descriptor.
    pub tdt:                        Volatile<u32>,          // 0x3818
}

pub const REG_CTRL:                 u32 = 0x0000;
pub const REG_STATUS:               u32 = 0x0008;
pub const REG_EEPROM:               u32 = 0x0014;
pub const REG_CTRL_EXT:             u32 = 0x0018;
pub const REG_IMASK:                u32 = 0x00D0;
pub const REG_RCTRL:                u32 = 0x0100;
pub const REG_RXDESCLO:             u32 = 0x2800;
pub const REG_RXDESCHI:             u32 = 0x2804;
pub const REG_RXDESCLEN:            u32 = 0x2808;
pub const REG_RXDESCHEAD:           u32 = 0x2810;
pub const REG_RXDESCTAIL:           u32 = 0x2818;

pub const REG_TCTRL:                u32 = 0x0400;
pub const REG_TXDESCLO:             u32 = 0x3800;
pub const REG_TXDESCHI:             u32 = 0x3804;
pub const REG_TXDESCLEN:            u32 = 0x3808;
pub const REG_TXDESCHEAD:           u32 = 0x3810;
pub const REG_TXDESCTAIL:           u32 = 0x3818;

/// RX Delay Timer Register
pub const REG_RDTR:                 u32 = 0x2820;    
/// RX Descriptor Control  
pub const REG_RXDCTL:               u32 = 0x3828;    
/// RX Int. Absolute Delay Timer  
pub const REG_RADV:                 u32 = 0x282C; 
/// RX Small Packet Detect Interrupt     
pub const REG_RSRPD:                u32 = 0x2C00;      
 
pub const REG_MTA:                  u32 = 0x5200; 
pub const REG_CRCERRS:              u32 = 0x4000;

/// Transmit Inter Packet Gap
pub const REG_TIPG:                 u32 = 0x0410;    
/// set link up  
pub const ECTRL_SLU:                u32 = 0x40;        

// CTRL commands
pub const CTRL_LRST:                u32 = 1 << 3;
pub const CTRL_ILOS:                u32 = 1 << 7;
pub const CTRL_VME:                 u32 = 1 << 30; 
pub const CTRL_PHY_RST:             u32 = 1 << 31;

// RCTL commands
/// Receiver Enable
pub const RCTL_EN:                  u32 = 1 << 1;    
/// Store Bad Packets
pub const RCTL_SBP:                 u32 = 1 << 2;   
/// Unicast Promiscuous Enabled 
pub const RCTL_UPE:                 u32 = 1 << 3;  
/// Multicast Promiscuous Enabled  
pub const RCTL_MPE:                 u32 = 1 << 4;    
/// Long Packet Reception Enable
pub const RCTL_LPE:                 u32 = 1 << 5;    
/// No Loopback
pub const RCTL_LBM_NONE:            u32 = 0 << 6;    
/// PHY or external SerDesc loopback
pub const RCTL_LBM_PHY:             u32 = 3 << 6;    
/// Free Buffer Threshold is 1/2 of RDLEN
pub const RTCL_RDMTS_HALF:          u32 = 0 << 8;    
/// Free Buffer Threshold is 1/4 of RDLEN
pub const RTCL_RDMTS_QUARTER:       u32 = 1 << 8;    
/// Free Buffer Threshold is 1/8 of RDLEN
pub const RTCL_RDMTS_EIGHTH:        u32 = 2 << 8;    
/// Multicast Offset - bits 47:36
pub const RCTL_MO_36:               u32 = 0 << 12;   
/// Multicast Offset - bits 46:35
pub const RCTL_MO_35:               u32 = 1 << 12;   
/// Multicast Offset - bits 45:34
pub const RCTL_MO_34:               u32 = 2 << 12;   
/// Multicast Offset - bits 43:32
pub const RCTL_MO_32:               u32 = 3 << 12;   
/// Broadcast Accept Mode
pub const RCTL_BAM:                 u32 = 1 << 15;   
/// VLAN Filter Enable
pub const RCTL_VFE:                 u32 = 1 << 18;   
/// Canonical Form Indicator Enable
pub const RCTL_CFIEN:               u32 = 1 << 19;   
/// Canonical Form Indicator Bit Value
pub const RCTL_CFI:                 u32 = 1 << 20;   
/// Discard Pause Frames
pub const RCTL_DPF:                 u32 = 1 << 22;   
/// Pass MAC Control Frames
pub const RCTL_PMCF:                u32 = 1 << 23;   
/// Strip Ethernet CRC
pub const RCTL_SECRC:               u32 = 1 << 26;   
 
// Buffer Sizes
pub const RCTL_BSIZE_256:           u32 = 3 << 16;
pub const RCTL_BSIZE_512:           u32 = 2 << 16;
pub const RCTL_BSIZE_1024:          u32 = 1 << 16;
pub const RCTL_BSIZE_2048:          u32 = 0 << 16;
pub const RCTL_BSIZE_4096:          u32 = (3 << 16) | (1 << 25);
pub const RCTL_BSIZE_8192:          u32 = (2 << 16) | (1 << 25);
pub const RCTL_BSIZE_16384:         u32 = (1 << 16) | (1 << 25);
 
 
// TCTL commands
/// Transmit Enable
pub const TCTL_EN:                  u32 = 1 << 1;    
/// Pad Short Packets
pub const TCTL_PSP:                 u32 = 1 << 3;   
/// Collision Threshold 
pub const TCTL_CT_SHIFT:            u32 = 4;           
/// Collision Distance
pub const TCTL_COLD_SHIFT:          u32 = 12;         
/// Software XOFF Transmission 
pub const TCTL_SWXOFF:              u32 = 1 << 22;   
/// Re-transmit on Late Collision
pub const TCTL_RTLC:                u32 = 1 << 24;   
 

