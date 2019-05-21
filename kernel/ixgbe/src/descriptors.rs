use bit_field::BitField;
use volatile::Volatile;

/// Legacy Receive Descriptor
#[repr(C,packed)]
pub struct LegacyRxDesc {
    pub phys_addr: Volatile<u64>,      
    pub length: Volatile<u16>,
    pub checksum: Volatile<u16>,
    pub status: Volatile<u8>,
    pub errors: Volatile<u8>,
    pub special: Volatile<u16>,
}
use core::fmt;
impl fmt::Debug for LegacyRxDesc {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{{addr: {:#X}, length: {}, checksum: {}, status: {}, errors: {}, special: {}}}",
                    self.phys_addr.read(), self.length.read(), self.checksum.read(), self.status.read(), self.errors.read(), self.special.read())
    }
}

/// Legacy Transmit Descriptor
/// TODO: Replace with advanced receive descriptor
#[repr(C,packed)]
pub struct LegacyTxDesc {
    pub phys_addr: Volatile<u64>,
    pub length: Volatile<u16>,
    pub cso: Volatile<u8>,
    pub cmd: Volatile<u8>,
    pub status: Volatile<u8>,
    pub css: Volatile<u8>,
    pub special : Volatile<u16>,
}

impl LegacyTxDesc {
    /// Initializes a transmit descriptor by clearing all of its values.
    pub fn init(&mut self) {
        self.phys_addr.write(0);
        self.length.write(0);
        self.cso.write(0);
        self.cmd.write(0);
        self.status.write(0);
        self.css.write(0);
        self.special.write(0);
    }
}

impl fmt::Debug for LegacyTxDesc {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{{addr: {:#X}, length: {}, cso: {}, cmd: {}, status: {}, css: {}, special: {}}}",
                    self.phys_addr.read(), self.length.read(), self.cso.read(), self.cmd.read(), self.status.read(), self.css.read(), self.special.read())
    }
}

/// Advanced Receive Descriptor (7.1.6)
/// It has 2 modes: Read and WriteBack. There is one receive descriptor per receive buffer that can be converted between these 2 forms.
/// Read contains the addresses that the driver writes. 
/// WriteBack contains the status bits that the hardware writes to it on receiving a packet,
#[repr(C, packed)]
pub struct AdvancedReceiveDescriptor {
    /// the packet buffer address
    upper:  Volatile<u64>,
    /// the header buffer address
    lower:  Volatile<u64>,
}

impl AdvancedReceiveDescriptor {
    /// initialize the Receive descriptor in Read mode by setting the packet and header physical address.
    pub fn init (&mut self, packet_buffer_addres: u64, header_buffer_address: u64) {
        self.upper.write(packet_buffer_addres);
        self.lower.write(header_buffer_address);
    }

    /*** Functions for Read format ***/

    pub fn set_packet_buffer_address(&mut self, val: u64){
        self.upper.write(val);
    }

    pub fn set_header_buffer_address(&mut self, val: u64){
        self.lower.write(val);
    }

    pub fn get_packet_buffer_address(&self) -> u64{
        self.upper.read() 
    }

    pub fn get_header_buffer_address(&self) -> u64{
        self.lower.read() 
    }

    /*** functions for Write Back format ***/

    pub fn get_rss_hash(&self) -> u64{
        self.upper.read().get_bits(32..63) 
    }

    pub fn get_sph(&self) -> bool{
        self.upper.read().get_bit(31) 
    }
    
    pub fn get_hdr_len(&self) -> u64{
        self.upper.read().get_bits(21..30) 
    }

    pub fn get_rsccnt(&self) -> u64{
        self.upper.read().get_bits(17..20) 
    }

    pub fn get_packet_type(&self) -> u64{
        self.upper.read().get_bits(4..16) 
    }

    pub fn get_rss_type(&self) -> u64{
        self.upper.read().get_bits(0..3) 
    }

    pub fn get_vlan_tag(&self) -> u64{
        self.lower.read().get_bits(48..63) 
    }

    pub fn get_pkt_len(&self) -> u64{
        self.lower.read().get_bits(32..47) 
    }

    pub fn get_ext_error(&self) -> u64{
        self.lower.read().get_bits(20..31) 
    }

    pub fn get_ext_status(&self) -> u64{
        self.lower.read().get_bits(0..19) 
    }
}

impl fmt::Debug for AdvancedReceiveDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{{Packet buffer address: {:#X}, Packet header address: {:#X}}}",
            self.upper.read(), self.lower.read())
    }
}

// impl From<AdvancedReceiveDescriptorWB> for AdvancedReceiveDescriptorR {

//     fn from (original: AdvancedReceiveDescriptorWB) -> AdvancedReceiveDescriptorR {
//         AdvancedReceiveDescriptorR {
//             upper: original.upper,
//             lower: original.lower,
//         }
//     }

// }

// /// Advanced Receive Descriptor ( Section 7.1.6 of 82599 datasheet)
// /// It has 2 modes: Read and WriteBack. There is one receive descriptor per receive buffer that can be converted between these 2 forms.
// /// WriteBack contains the status bits that the hardware writes to it on receiving a packet,
// #[repr(C, packed)]
// pub struct AdvancedReceiveDescriptorWB {
//     upper:  Volatile<u64>,
//     lower:  Volatile<u64>,
// }

// impl AdvancedReceiveDescriptorWB {

//     pub fn get_rss_hash(&self) -> u64{
//         self.upper.read().get_bits(32..63) 
//     }

//     pub fn get_sph(&self) -> bool{
//         self.upper.read().get_bit(31) 
//     }
    
//     pub fn get_hdr_len(&self) -> u64{
//         self.upper.read().get_bits(21..30) 
//     }

//     pub fn get_rsccnt(&self) -> u64{
//         self.upper.read().get_bits(17..20) 
//     }

//     pub fn get_packet_type(&self) -> u64{
//         self.upper.read().get_bits(4..16) 
//     }

//     pub fn get_rss_type(&self) -> u64{
//         self.upper.read().get_bits(0..3) 
//     }

//     pub fn get_vlan_tag(&self) -> u64{
//         self.lower.read().get_bits(48..63) 
//     }

//     pub fn get_pkt_len(&self) -> u64{
//         self.lower.read().get_bits(32..47) 
//     }

//     pub fn get_ext_error(&self) -> u64{
//         self.lower.read().get_bits(20..31) 
//     }

//     pub fn get_ext_status(&self) -> u64{
//         self.lower.read().get_bits(0..19) 
//     }
// }

// impl fmt::Debug for AdvancedReceiveDescriptorWB {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//         write!(f, "{{rss_hash: {:#X}, sph: {}, hdr_len: {:#X}, rsccnt: {:#X}, packet_type: {:#X}, rss_type: {:#X}, vlan_tag: {:#X}, pkt_len: {:#X}, ext_error: {:#X}, ext_status: {:#X}}}",
//             self.get_rss_hash(), self.get_sph(), self.get_hdr_len(), self.get_rsccnt(), self.get_packet_type(), self.get_rss_type(), self.get_vlan_tag(), self.get_pkt_len(), self.get_ext_error(), self.get_ext_status())
//     }
// }

// impl From<AdvancedReceiveDescriptorR> for AdvancedReceiveDescriptorWB {

//     fn from (original: AdvancedReceiveDescriptorR) -> AdvancedReceiveDescriptorWB {
//         AdvancedReceiveDescriptorWB {
//             upper: original.upper,
//             lower: original.lower,
//         }
//     }

// }

