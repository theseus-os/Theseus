
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
pub const REG_EITR:                 u32 = 0x820; // Interrupt throttle registers

pub const REG_MRQC:                 u32 = 0xEC80;
pub const REG_ETQF:                 u32 = 0x5128;
pub const REG_ETQS:                 u32 = 0xEC00;

/******************/

///CTRL commands
pub const CTRL_LRST:                u32 = (1<<3); 
pub const CTRL_RST:                 u32 = (1<<26);

/// RCTL commands
pub const BSIZEPACKET_8K:           u32 = 8;
pub const BSIZEHEADER_256B:         u32 = 4;
pub const DESCTYPE_LEG:             u32 = 0;
pub const DESCTYPE_ADV_1BUFFER:     u32 = 1;
pub const DESCTYPE_ADV_HS:          u32 = 2;
pub const RX_Q_ENABLE:              bool = true;

// RSS commands
pub const RXCSUM_PCSD:              u32 = 1 << 13; 
pub const MRQC_MRQE_RSS:            u32 = 1; // set bits 0..3 in MRQC
pub const MRQC_TCPIPV4:             u32 = 1 << 16; 
pub const MRQC_IPV4:                u32 = 1 << 17; 
pub const MRQC_IPV6:                u32 = 1 << 20;
pub const MRQC_TCPIPV6:             u32 = 1 << 21;  
pub const MRQC_UDPIPV4:             u32 = 1 << 22; 
pub const MRQC_UDPIPV6:             u32 = 1 << 23;  
 
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
pub const TX_Q_ENABLE:              bool = true;
pub const TE:                       u32  = 1; //Transmit Enable

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



#[repr(C)]
pub struct IntelIxgbeRegisters {
    pub ctrl:                       Volatile<u32>,          // 0x0
    _padding0:                      [u8; 4],                // 0x4 - 0x7
    
    pub status:                     ReadOnly<u32>,          // 0x8
    _padding1:                      [u8; 2036],             // 0xC - 0x7FF

    pub eicr:                       Volatile<u32>,          // 0x800
    _padding2:                      [u8; 4],                // 0x804 - 0x807

    pub eics:                       WriteOnly<u32>,         // 0x808 // set bits in eicr register 
    _padding3:                      [u8; 4],                // 0x80C - 0x80F

    pub eiac:                       Volatile<u32>,          // 0x810; // enables auto-clear
    _padding4:                      [u8; 12],               // 0x814 - 0x81F 
    
    pub eitr:                       RegisterArray24,        // 0x820 - 0x87F; // first 24 Interrupt throttle registers

    pub eims:                       Volatile<u32>,          // 0x880; // enables interrupt in eicr register
    _padding5:                      [u8; 4],                // 0x884 - 0x887

    pub eimc:                       WriteOnly<u32>,         // 0x888; // clears bit in eims reg, disabling that interrupt
    _padding6:                      [u8; 4],                // 0x88C - 0x88F     

    pub eiam:                       Volatile<u32>,          // 0x890; // enables auto set and clear
    _padding7:                      [u8; 4],                // 0x894 - 0x897

    pub gpie:                       Volatile<u32>,          // 0x898; // enable clear on read
    _padding8:                      [u8; 100],              // 0x89C - 0x8FF

    pub ivar:                       RegisterArray64,        // 0x900 - 0x9FF  // maps interrupt causes from Rx and Tx queues to eicr entries (0x900 + 4*n, n = 0..63)
    _padding9:                      [u8; 1536],             // 0xA00 - 0xFFF

    pub rx_regs1:                   RegisterArrayRx,        // 0x1000 - 0x1FFF, for 64 queues
    _padding10:                     [u8; 3840],             // 0x2000 - 0x2EFF

    pub rdrxctl:                    Volatile<u32>,          // 0x2F00;
    _padding11:                     [u8; 252],              // 0x2F04 - 0x2FFF

    pub rxctrl:                     Volatile<u32>,          // 0x3000;
    _padding12:                     [u8; 508],              // 0x3004 - 0x31FF

    pub fcttv:                      RegisterArray4,         // 0x3200 - 0x320F, +4*n with n=0..3
    _padding13:                     [u8; 16],               // 0x3210 - 0x321F

    pub fcrtl:                      RegisterArray8,         // 0x3220 - 0x323F, +4*n with n=0..7
    _padding14:                     [u8; 32],               // 0x3240 - 0x325F 

    pub fcrth:                      RegisterArray8,         // 0x3260 - 0x327F, +4*n with n=0..7
    _padding15:                     [u8; 32],               // 0x3280 - 0x329F

    pub fcrtv:                      Volatile<u32>,          // 0x32A0;
    _padding16:                     [u8; 2652],             // 0x32A4 - 0x3CFF

    pub fccfg:                      Volatile<u32>,          // 0x3D00;
    _padding17:                     [u8; 1340],             // 0x3D04 - 0x423F

    pub hlreg0:                     Volatile<u32>,          // 0x4240;
    _padding18:                     [u8; 92],               // 0x4244 - 0x429F

    pub autoc:                      Volatile<u32>,          // 0x42A0;

    pub links:                      Volatile<u32>,          // 0x42A4;

    pub autoc2:                     Volatile<u32>,          // 0x42A8;
    _padding19:                     [u8; 1620],             // 0x42AC - 0x48FF

    pub rttdcs:                     Volatile<u32>,          // 0x4900;
    _padding20:                     [u8; 380],              // 0x4904 - 0x4A7F

    pub dmatxctl:                   Volatile<u32>,          // 0x4A80;
    _padding21:                     [u8; 4],                // 0x4A84 - 0x4A87
    
    pub dtxtcpflgl:                 Volatile<u32>,          // 0x4A88;
    
    pub dtxtcpflgh:                 Volatile<u32>,          // 0x4A8C;
    _padding22:                     [u8; 1392],             // 0x4A90 - 0x4FFF

    pub rxcsum:                     Volatile<u32>,          // 0x5000
    _paddin22a:                     [u8; 124],              // 0x5004 - 0x507F

    pub fctrl:                      Volatile<u32>,          // 0x5080;
    _padding23:                     [u8; 164],              // 0x5084 - 0x5127

    pub etqf:                       RegisterArray8,         // 0x5128 - 0x5147;
    _padding24:                     [u8; 3768],             // 0x5148 - 0x5FFF

    pub tx_regs:                    RegisterArrayTx,        // 0x6000 - 0x7FFF, end at 0x40 * 128, 128 queues
    _padding25:                     [u8; 8704],             // 0x8000 - 0xA1FF

    pub ral:                        Volatile<u32>,          // 0xA200;
    
    pub rah:                        Volatile<u32>,          // 0xA204;
    _padding26:                     [u8; 11768],            // 0xA208 - 0xCFFF

    pub rx_regs2:                   RegisterArrayRx,        // 0xD000 - 0xDFFF, for 64 queues
    _padding27:                     [u8; 2816],             // 0xE000 - 0xEAFF

    pub reta:                       RegisterArray32,        // 0xEB00 - 0xEB7F

    pub rssrk:                      RegisterArray10,        // 0xEB80 - 0xEBA7
    _padding27a:                    [u8; 88],               // 0xEBA8 - 0xEBFF

    pub etqs:                       RegisterArray8,         // 0xEC00 - 0xEC1F;
    _padding28:                     [u8; 96],               // 0xEC20 - 0xEC7F

    pub mrqc:                       Volatile<u32>,          // 0xEC80;
    _padding29:                     [u8; 5008],             // 0xEC84 - 0x10013

    pub eerd:                       Volatile<u32>,          // 0x10014;
    _padding30:                     [u8; 65512],            // 0x10018 - 0x1FFFF
} //128 KB

#[repr(C)]
pub struct RegisterArray4 {
    pub reg:                            [Volatile<u32>;4],
}

#[repr(C)]
pub struct RegisterArray8 {
    pub reg:                            [Volatile<u32>;8],
}

#[repr(C)]
pub struct RegisterArray10 {
    pub reg:                            [Volatile<u32>;10],
}

#[repr(C)]
pub struct RegisterArray32 {
    pub reg:                            [Volatile<u32>;32],
}

#[repr(C)]
pub struct RegisterArray64 {
    pub reg:                            [Volatile<u32>;64],
}

#[repr(C)]
pub struct RegisterArray24 {
    pub reg:                           [Volatile<u32>;24],
}

#[repr(C)]
pub struct RegisterArray104 {
    pub reg:                           [Volatile<u32>;104],
}

//size 0x40
#[repr(C)]
pub struct RegistersTx {
    pub tdbal:                          Volatile<u32>,          // 0x6000
    pub tdbah:                          Volatile<u32>,          // 0x6004
    pub tdlen:                          Volatile<u32>,          // 0x6008
    pub dca_txctrl:                     Volatile<u32>,          // 0x600C
    pub tdh:                            Volatile<u32>,          // 0x6010
    _padding0:                      [u8; 4],                // 0x6014 - 0x6017
    pub tdt:                            Volatile<u32>,          // 0x6018
    _padding1:                      [u8; 12],               // 0x601C - 0x6027
    pub txdctl:                         Volatile<u32>,          // 0x6028
    _padding2:                      [u8; 12],               // 0x602C - 0x6037
    pub tdwbal:                         Volatile<u32>,          // 0x6038
    pub tdwbah:                         Volatile<u32>,          // 0x603C
}

#[repr(C)]
pub struct RegisterArrayTx {
    pub tx_queue:                          [RegistersTx; 128],
}

//size 0x40
#[repr(C)]
pub struct RegistersRx {
    pub rdbal:                          Volatile<u32>,          // 0x1000;
    pub rdbah:                          Volatile<u32>,          // 0x1004;
    pub rdlen:                          Volatile<u32>,          // 0x1008;
    _padding0:                      [u8;4],                 // 0x100C - 0x100F
    pub rdh:                            Volatile<u32>,          // 0x1010;
    pub srrctl:                         Volatile<u32>,          // 0x1014; //specify descriptor type
    pub rdt:                            Volatile<u32>,          // 0x1018;
    _padding1:                      [u8;12],                // 0x101C - 0x1027
    pub rxdctl:                         Volatile<u32>,          // 0x1028;
    _padding2:                      [u8;20],                // 0x102C - 0x103F                                            
}

#[repr(C)]
pub struct RegisterArrayRx {
    pub rx_queue:                          [RegistersRx; 64],
}
