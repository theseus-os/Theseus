use bit_field::BitField;

/// struct to represent receive descriptors
#[repr(C,packed)]
pub struct e1000_rx_desc {
        pub addr: u64,      
        pub length: u16,
        pub checksum: u16,
        pub status: u8,
        pub errors: u8,
        pub special: u16,
}
use core::fmt;
impl fmt::Debug for e1000_rx_desc {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "{{addr: {:#X}, length: {}, checksum: {}, status: {}, errors: {}, special: {}}}",
                        self.addr, self.length, self.checksum, self.status, self.errors, self.special)
        }
}

/// struct to represent transmission descriptors
#[repr(C,packed)]
pub struct e1000_tx_desc {
        pub addr: u64,
        pub length: u16,
        pub cso: u8,
        pub cmd: u8,
        pub status: u8,
        pub css: u8,
        pub special : u16,
}
impl fmt::Debug for e1000_tx_desc {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "{{addr: {:#X}, length: {}, cso: {}, cmd: {}, status: {}, css: {}, special: {}}}",
                        self.addr, self.length, self.cso, self.cmd, self.status, self.css, self.special)
        }
}

#[repr(packed)]
#[derive(Debug)]
pub struct AdvancedReceiveDescriptorRead {
    pub upper:  u64,
    pub lower:  u64,
}

impl AdvancedReceiveDescriptorRead {
    pub fn set_packet_buffer_address(&mut self, val: u64){
        self.upper = val;
    }

    pub fn set_header_buffer_address(&mut self, val: u64){
        self.lower = val;
    }

    pub fn get_packet_buffer_address(&self) -> u64{
        self.upper 
    }

    pub fn get_header_buffer_address(&self) -> u64{
        self.lower 
    }
}
   

#[repr(packed)]
#[derive(Debug)]
pub struct AdvancedReceiveDescriptorWriteBack {
    upper:  u64,
    lower:  u64,
}

impl AdvancedReceiveDescriptorWriteBack {
     pub fn get_rss_hash(&self) -> u64{
        self.upper.get_bits(32..63) 
    }

    pub fn get_sph(&self) -> bool{
        self.upper.get_bit(31) 
    }
    
    pub fn get_hdr_len(&self) -> u64{
        self.upper.get_bits(21..30) 
    }

    pub fn get_rsccnt(&self) -> u64{
        self.upper.get_bits(17..20) 
    }

    pub fn get_packet_type(&self) -> u64{
        self.upper.get_bits(4..16) 
    }

    pub fn get_rss_type(&self) -> u64{
        self.upper.get_bits(0..3) 
    }

    pub fn get_vlan_tag(&self) -> u64{
        self.lower.get_bits(48..63) 
    }

    pub fn get_pkt_len(&self) -> u64{
        self.lower.get_bits(32..47) 
    }

    pub fn get_ext_error(&self) -> u64{
        self.lower.get_bits(20..31) 
    }

    pub fn set_ext_status(&mut self, value: u64){
        self.lower.set_bits(0..19, value); 
    }
    pub fn get_ext_status(&self) -> u64{
        self.lower.get_bits(0..19) 
    }
}
