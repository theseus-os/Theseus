
pub const INTEL_VEND:               u16 = 0x8086;  // Vendor ID for Intel 
pub const INTEL_82599:              u16 = 0x10FB;  // Device ID for the e1000 Qemu, Bochs, and VirtualBox emmulated NICs

pub const REG_CTRL:                 u32 = 0x0000;
pub const REG_STATUS:               u32 = 0x0008;

pub const REG_FCTTV:                u32 = 0x3200; //+4*n with n=0..3
pub const REG_FCRTL:                u32 = 0x3220; //+4*n with n=0..7
pub const REG_FCRTH:                u32 = 0x3260; //+4*n with n=0..7
pub const REG_FCRTV:                u32 = 0x32A0;
pub const REG_FCCFG:                u32 = 0x3D00;

pub const REG_RDRXCTL:              u32 = 0x2F00;
pub const DMAIDONE:                 u32 = 1<<3;

pub const REG_RAL:                  u32 = 0xA200;
pub const REG_RAH:                  u32 = 0xA204;

pub const REG_AUTOC:                u32 = 0x42A0;
pub const REG_AUTOC2:               u32 = 0x42A8;
pub const REG_LINKS:                u32 = 0x42A4;

pub const AUTOC_FLU:                u32 = 1;
pub const AUTOC_LMS:                u32 = 6<<13; //KX/KX4//KR
pub const AUTOC_10G_PMA_PMD_PAR:    u32 = 1<<7;
pub const AUTOC2_10G_PMA_PMD_PAR:   u32 = 0<<8|0<<7; 
pub const AUTOC_RESTART_AN:         u32 = 1<<12;

pub const REG_EERD:                 u32 = 0x10014;

pub const REG_HLREG0:               u32 = 0x4240;
pub const REG_DMATXCTL:             u32 = 0x4A80;
pub const REG_DTXTCPFLGL:           u32 = 0x4A88;
pub const REG_DTXTCPFLGH:           u32 = 0x4A8C;
pub const REG_DCATXCTRL:            u32 = 0x600C;
pub const REG_RTTDCS:               u32 = 0x4900;

pub const REG_TDBAL:                u32 = 0x6000;
pub const REG_TDBAH:                u32 = 0x6004;
pub const REG_TDLEN:                u32 = 0x6008;
pub const REG_TDH:                  u32 = 0x6010;
pub const REG_TDT:                  u32 = 0x6018;
pub const REG_TXDCTL:               u32 = 0x6028;

pub const REG_RDBAL:                u32 = 0x1000;
pub const REG_RDBAH:                u32 = 0x1004;
pub const REG_RDLEN:                u32 = 0x1008;
pub const REG_RDH:                  u32 = 0x1010;
pub const REG_RDT:                  u32 = 0x1018;
pub const REG_RXDCTL:               u32 = 0x1028;
pub const REG_SRRCTL:               u32 = 0x1014; //specify descriptor type
pub const REG_RXCTRL:               u32 = 0x3000;
pub const REG_FCTRL:                u32 = 0x5080;

//Interrupt registers
pub const REG_EICR:                 u32 = 0x800;
pub const REG_EICS:                 u32 = 0x808; // set bits in eicr register 
pub const REG_EIMS:                 u32 = 0x880; // enables interrupt in eicr register
pub const REG_EIMC:                 u32 = 0x888; // clears bit in eims reg, disabling that interrupt
pub const REG_EIAC:                 u32 = 0x810; // enables auto-clear
pub const REG_EIAM:                 u32 = 0x890; // enables auto set and clear
pub const REG_IVAR:                 u32 = 0x900; // maps interrupt causes from Rx and Tx queues to eicr entries (0x900 + 4*n, n = 0..63)
pub const REG_GPIE:                 u32 = 0x898; // enable clear on read

pub const REG_MRQC:                 u32 = 0xEC80;
pub const REG_ETQF:                 u32 = 0x5128;
pub const REG_ETQS:                 u32 = 0xEC00;

/******************/

///CTRL commands
pub const CTRL_LRST:                u32 = (1<<3); 
pub const CTRL_RST:                 u32 = (1<<26);

/// RCTL commands
pub const BSIZEPACKET_8K:           u32 = 8;
pub const DESCTYPE_LEG:             u32 = 0;
pub const DESCTYPE_ADV_1BUFFER:     u32 = 1;
pub const RX_Q_ENABLE:              bool = true;

pub const RSS_ONLY:                 u32 = 1;
pub const RSS_UDPIPV4:              u32 = 0x40; // bit 22
 
/// Buffer Sizes
pub const RCTL_BSIZE_256:           u32 = (3 << 16);
pub const RCTL_BSIZE_512:           u32 = (2 << 16);
pub const RCTL_BSIZE_1024:          u32 = (1 << 16);
pub const RCTL_BSIZE_2048:          u32 = (0 << 16);
pub const RCTL_BSIZE_4096:          u32 = ((3 << 16) | (1 << 25));
pub const RCTL_BSIZE_8192:          u32 = ((2 << 16) | (1 << 25));
pub const RCTL_BSIZE_16384:         u32 = ((1 << 16) | (1 << 25));
 
 
/// Transmit Command
 
pub const CMD_EOP:                  u32 = (1 << 0);    // End of Packet
pub const CMD_IFCS:                 u32 = (1 << 1);   // Insert FCS
pub const CMD_IC:                   u32 = (1 << 2);    // Insert Checksum
pub const CMD_RS:                   u32 = (1 << 3);   // Report Status
pub const CMD_RPS:                  u32 = (1 << 4);   // Report Packet Sent
pub const CMD_VLE:                  u32 = (1 << 6);    // VLAN Packet Enable
pub const CMD_IDE:                  u32 = (1 << 7);    // Interrupt Delay Enable
 
 
/// TCTL commands
 
pub const TCTL_EN:                  u32 = (1 << 1);    // Transmit Enable
pub const TCTL_PSP:                 u32 = (1 << 3);    // Pad Short Packets
pub const TCTL_CT_SHIFT:            u32 = 4;          // Collision Threshold
pub const TCTL_COLD_SHIFT:          u32 = 12;          // Collision Distance
pub const TCTL_SWXOFF:              u32 = (1 << 22);   // Software XOFF Transmission
pub const TCTL_RTLC:                u32 = (1 << 24);   // Re-transmit on Late Collision
 
pub const TSTA_DD:                  u32 = (1 << 0);    // Descriptor Done
pub const TSTA_EC:                  u32 = (1 << 1);    // Excess Collisions
pub const TSTA_LC:                  u32 = (1 << 2);    // Late Collision
pub const LSTA_TU:                  u32 = (1 << 3);    // Transmit Underrun