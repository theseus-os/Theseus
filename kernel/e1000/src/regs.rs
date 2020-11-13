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
 

