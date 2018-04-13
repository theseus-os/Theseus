//use super::E1000_NIC;
use super::E1000E_NIC;



pub fn test_nic_driver(_: Option<u64>) {
    debug!("TESTING e1000 NIC DRIVER!!");
    dhcp_request_packet();
}


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

//should test packet transmission and reception as QEMU DHCP server will respond

//will only b able to see this Tx message in netdump.pcap if user is not mentioned in QEMU flags of Makefile
//QEMU_FLAGS += -net nic,vlan=0,model=e1000,macaddr=00:0b:82:01:fc:42 -net dump,file=netdump.pcap

//will only receive a response if user is mentioned in qemu flags
//QEMU_FLAGS += -net nic,vlan=1,model=e1000,macaddr=00:0b:82:01:fc:42 -net user,vlan=1 -net dump,file=netdump.pcap

pub fn dhcp_request_packet(){
    //let mut e1000_nc = E1000_NIC.lock();
    let mut e1000_nc = E1000E_NIC.lock();
    let packet:[u8;314] = [0xff,0xff,0xff,0xff,0xff,0xff,0x00,0x0b,0x82,0x01,0xfc,0x42,0x08,0x00,0x45,0x00,
                               0x01,0x2c,0xa8,0x36,0x00,0x00,0xfa,0x11,0x17,0x8b,0x00,0x00,0x00,0x00,0xff,0xff,
                               0xff,0xff,0x00,0x44,0x00,0x43,0x01,0x18,0x59,0x1f,0x01,0x01,0x06,0x00,0x00,0x00,
                               0x3d,0x1d,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x0b,0x82,0x01,0xfc,0x42,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x63,0x82,0x53,0x63,0x35,0x01,0x01,0x3d,0x07,0x01,
                               0x00,0x0b,0x82,0x01,0xfc,0x42,0x32,0x04,0x00,0x00,0x00,0x00,0x37,0x04,0x01,0x03,
                               0x06,0x2a,0xff,0x00,0x00,0x00,0x00,0x00,0x00,0x00];
        let addr = &packet as *const u8;
        let addr: usize = addr as usize;
        let length: u16 = 314;
        let _result = e1000_nc.send_packet(addr, length);
}