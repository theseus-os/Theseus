#[repr(C,packed)]
pub struct arp_packet{
    pub dest1: u16, //set to broadcast ff:ff:...
    pub dest2: u16,
    pub dest3: u16,
    pub source1: u16,
    pub source2: u16,
    pub source3: u16,
    pub packet_type:   u16,
    pub h_type: u16,
    pub p_type: u16,
    pub hlen:   u8,
    pub plen:   u8,
    pub oper:   u16,
    pub sha1:   u16, //sender hw address, first 2 bytes
    pub sha2:   u16, // ", next 2 bytes
    pub sha3:   u16, // ", last 2 bytes
    pub spa1:   u16, // sender protocol address, first 2 B
    pub spa2:   u16, // ", last 2 B
    pub tha1:   u16, //target ", first
    pub tha2:   u16, // ", next
    pub tha3:   u16, // ", last
    pub tpa1:   u16, // ", first
    pub tpa2:   u16, // ", last
} 