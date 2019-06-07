use bit_field::BitField;
use volatile::{Volatile, ReadOnly};
use memory::PhysicalAddress;

/// The types of descriptors that can be used by Intel ethernet cards
/// Check the datasheet to see which ones your device supports
pub enum DescriptorType {
    Legacy,
    Advanced,
}

/// This struct is a Legacy Receive Descriptor
/// There is one instance of this struct per receive buffer. 
#[repr(C)]
pub struct LegacyRxDesc {
    /// The starting physical address of the receive buffer
    pub phys_addr:  Volatile<u64>,      
    /// Length of the receive buffer in bytes
    pub length:     ReadOnly<u16>,
    pub checksum:   ReadOnly<u16>,
    pub status:     Volatile<u8>,
    pub errors:     ReadOnly<u8>,
    pub special:    ReadOnly<u16>,
}

impl LegacyRxDesc {
    /// Initializes a receive descriptor by clearing its status 
    /// and setting the descriptor's physical address.
    /// 
    /// # Arguments: 
    /// * 'rx_buf_paddr': the starting physical address of the receive buffer
    pub fn init(&mut self, rx_buf_paddr: PhysicalAddress) {
        self.phys_addr.write(rx_buf_paddr.value() as u64);
        self.status.write(0);
    }
}

use core::fmt;
impl fmt::Debug for LegacyRxDesc {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{{addr: {:#X}, length: {}, checksum: {}, status: {}, errors: {}, special: {}}}",
                    self.phys_addr.read(), self.length.read(), self.checksum.read(), self.status.read(), self.errors.read(), self.special.read())
    }
}

/// This struct is a Legacy Transmit Descriptor
/// There is one instance of this struct per buffer to be transmitted. 
#[repr(C,packed)]
pub struct LegacyTxDesc {
    /// The starting physical address of the transmit buffer
    pub phys_addr:  Volatile<u64>,
    /// Length of the transmit buffer in bytes
    pub length:     Volatile<u16>,
    pub cso:        Volatile<u8>,
    pub cmd:        Volatile<u8>,
    pub status:     Volatile<u8>,
    pub css:        Volatile<u8>,
    pub special :   Volatile<u16>,
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

/// Advanced Receive Descriptor used in the ixgbe driver (section 7.1.6 of 82599 datasheet)
/// It has 2 modes: Read and Write Back. There is one receive descriptor per receive buffer that can be converted between these 2 modes.
/// Read contains the addresses that the driver writes. 
/// Write Back contains information the hardware writes on receiving a packet,
#[repr(C)]
pub struct AdvancedReceiveDescriptor {
    /// the starting physcal address of the receive buffer for the packet
    pub packet_buffer_address:  Volatile<u64>,
    /// the starting physcal address of the receive buffer for the header
    /// this field will only be used if header splitting is enabled    
    pub header_buffer_address:  Volatile<u64>,
}

impl AdvancedReceiveDescriptor {
    /// Read mode function for the Advanced Receive Descriptor
    /// Sets the packet and header physical address.
    /// 
    /// # Arguments:
    /// * 'packet_buffer_address': the starting physical address of the receive buffer for the packet
    /// * 'header_buffer_address': the starting physical address of the receive buffer for the packet header if header splitting is enabled
    pub fn init (&mut self, packet_buffer_address: u64, header_buffer_address: Option<u64>) {
        self.packet_buffer_address.write(packet_buffer_address);
        self.header_buffer_address.write(header_buffer_address.unwrap_or(0));
    }

    /// Write Back mode function for the Advanced Receive Descriptor
    /// returns the packet type that was used for the Receive Side Scaling hash function
    pub fn get_rss_type(&self) -> u64{
        self.packet_buffer_address.read().get_bits(0..3) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor
    /// returns the packet type identified by the hardware
    pub fn get_packet_type(&self) -> u64{
        self.packet_buffer_address.read().get_bits(4..16) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor
    /// returns the number of Receive Side Coalesced packets that start in this descriptor
    pub fn get_rsccnt(&self) -> u64{
        self.packet_buffer_address.read().get_bits(17..20) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor
    /// returns the size of the packet header in bytes
    pub fn get_hdr_len(&self) -> u64{
        self.packet_buffer_address.read().get_bits(21..30) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor
    /// when set to 1b, indicates that the hardware has found the length of the header
    pub fn get_sph(&self) -> bool{
        self.packet_buffer_address.read().get_bit(31) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor
    /// Returns the Receive Side Scaling hash 
    pub fn get_rss_hash(&self) -> u64{
        self.packet_buffer_address.read().get_bits(32..63) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor
    /// Returns the Flow Director Filter ID if the packet matches a filter 
    pub fn get_fdf_id(&self) -> u64{
        self.packet_buffer_address.read().get_bits(32..63) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor
    /// Status information indicates whether a descriptor has been used 
    /// and whether the buffer is the last one for a packet
    pub fn get_ext_status(&self) -> u64{
        self.header_buffer_address.read().get_bits(0..19) 
    }
    
    /// Write Back mode function for the Advanced Receive Descriptor
    /// returns errors reported by hardware for different packet types
    pub fn get_ext_error(&self) -> u64{
        self.header_buffer_address.read().get_bits(20..31) 
    }
    
    /// Write Back mode function for the Advanced Receive Descriptor
    /// Returns the number of bytes posted to the packet buffer
    pub fn get_pkt_len(&self) -> u64{
        self.header_buffer_address.read().get_bits(32..47) 
    }
    
    /// Write Back mode function for the Advanced Receive Descriptor
    /// If the vlan header is stripped from the packet, then the 16 bits of the VLAN tag are posted here
    pub fn get_vlan_tag(&self) -> u64{
        self.header_buffer_address.read().get_bits(48..63) 
    }    
}

impl fmt::Debug for AdvancedReceiveDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{{Packet buffer address: {:#X}, Packet header address: {:#X}}}",
            self.packet_buffer_address.read(), self.header_buffer_address.read())
    }
}

