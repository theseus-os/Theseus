//! The different descriptor types used by Intel NICs.
//! Usually the newer NICs use more advanced descriptor types but retain support for older ones.
//! The difference in the types is the amount of packet information that is included in the descriptor, and the format.

use memory::PhysicalAddress;
use volatile::{Volatile, ReadOnly};
use bit_field::BitField;
use zerocopy::FromBytes;

// Transmit descriptor bits
/// Tx Command: End of Packet
pub const TX_CMD_EOP:                      u8 = 1 << 0;     
/// Tx Command: Insert MAC FCS
pub const TX_CMD_IFCS:                     u8 = 1 << 1;     
/// Tx Command: Insert Checksum
pub const TX_CMD_IC:                       u8 = 1 << 2;     
/// Tx Command: Report Status
pub const TX_CMD_RS:                       u8 = 1 << 3;     
/// Tx Command: Report Packet Sent
pub const TX_CMD_RPS:                      u8 = 1 << 4;     
/// Tx Command: Descriptor Extension (Advanced format)
pub const TX_CMD_DEXT:                     u8 = 1 << 5;  
/// Tx Command: VLAN Packet Enable
pub const TX_CMD_VLE:                      u8 = 1 << 6;     
/// Tx Command: Interrupt Delay Enable
pub const TX_CMD_IDE:                      u8 = 1 << 7;     
/// Tx Status: descriptor Done
pub const TX_STATUS_DD:                    u8 = 1 << 0;
/// Tx Descriptor Type: advanced
pub const TX_DTYP_ADV:                     u8 = 0x3 << 4;
/// Tx Descriptor paylen shift
/// The paylen is located at bit 46 in the upper 64 bits of the advanced Tx descriptor.
/// Since we have divided the upper 64 bits into 4 parts (u16,u8,u8,u32),
/// the paylen is then located at bit 14 of the upper 32 bits of the descriptor.
pub const TX_PAYLEN_SHIFT:                 u8 = 46 - 32; //(actual offset - offset of variable) 

// Receive descriptor bits 
/// Rx Status: Descriptor Done
pub const RX_STATUS_DD:                    u8 = 1 << 0;
/// Rx Status: End of Packet
pub const RX_STATUS_EOP:                   u8 = 1 << 1;


/// A trait for the minimum set of functions needed to receive a packet using one of Intel's receive descriptor types.
/// Receive descriptors contain the physical address where an incoming packet should be stored by the NIC,
/// as well as bits that are updated by the HW once the packet is received.
/// There is one receive descriptor per receive buffer. 
/// Receive functions defined in the Network_Interface_Card crate expect a receive descriptor to implement this trait.
pub trait RxDescriptor: FromBytes {
    /// Initializes a receive descriptor by clearing its status 
    /// and setting the descriptor's physical address.
    /// 
    /// # Arguments
    /// * `packet_buffer_address`: starting physical address of the receive buffer.
    fn init(&mut self, packet_buffer_address: PhysicalAddress);

    /// Updates the descriptor's physical address.
    /// 
    /// # Arguments
    /// * `packet_buffer_address`: starting physical address of the receive buffer.
    fn set_packet_address(&mut self, packet_buffer_address: PhysicalAddress);

    /// Clears the status bits of the descriptor.
    fn reset_status(&mut self);

    /// Returns true if the descriptor has a received packet copied to its buffer.
    fn descriptor_done(&self) -> bool;

    /// Returns true if the descriptor's packet buffer is the last in a frame.
    fn end_of_packet(&self) -> bool;

    /// The length of the packet in the descriptor's packet buffer.
    fn length(&self) -> u64;
}

/// A trait for the minimum set of functions needed to transmit a packet using one of Intel's transmit descriptor types.
/// Transmit descriptors contain the physical address where an outgoing packet is stored,
/// as well as bits that are updated by the HW once the packet is sent.
/// There is one transmit descriptor per transmit buffer.
/// Transmit functions defined in the Network_Interface_Card crate expect a transmit descriptor to implement this trait.
pub trait TxDescriptor: FromBytes {
    /// Initializes a transmit descriptor by clearing all of its values.
    fn init(&mut self);

    /// Updates the transmit descriptor to send the packet.
    /// We assume that one transmit descriptor will be used to send one packet.
    /// 
    /// # Arguments
    /// * `transmit_buffer_addr`: physical address of the transmit buffer. 
    /// * `transmit_buffer_length`: length of packet we want to send.
    fn send(&mut self, transmit_buffer_addr: PhysicalAddress, transmit_buffer_length: u16);

    /// Polls the Descriptor Done bit until the packet has been sent.
    fn wait_for_packet_tx(&self);
}



/// This struct is a Legacy Transmit Descriptor. 
/// It's the descriptor type used in older Intel NICs and the E1000 driver.
#[derive(FromBytes)]
#[repr(C)]
pub struct LegacyTxDescriptor {
    /// The starting physical address of the transmit buffer
    pub phys_addr:  Volatile<u64>,
    /// Length of the transmit buffer in bytes
    pub length:     Volatile<u16>,
    /// Checksum offset: where to insert the checksum from the start of the packet if enabled
    pub cso:        Volatile<u8>,
    /// Command bits
    pub cmd:        Volatile<u8>,
    /// Status bits
    pub status:     Volatile<u8>,
    /// Checksum start: where to begin computing the checksum, if enabled
    pub css:        Volatile<u8>,
    /// Vlan tags 
    pub vlan :      Volatile<u16>,
}

impl TxDescriptor for LegacyTxDescriptor {
    fn init(&mut self) {
        self.phys_addr.write(0);
        self.length.write(0);
        self.cso.write(0);
        self.cmd.write(0);
        self.status.write(0);
        self.css.write(0);
        self.vlan.write(0);
    }

    fn send(&mut self, transmit_buffer_addr: PhysicalAddress, transmit_buffer_length: u16) {
        self.phys_addr.write(transmit_buffer_addr.value() as u64);
        self.length.write(transmit_buffer_length);
        self.cmd.write(TX_CMD_EOP | TX_CMD_IFCS | TX_CMD_RPS | TX_CMD_RS); 
        self.status.write(0);
    }

    fn wait_for_packet_tx(&self) {
        while (self.status.read() & TX_STATUS_DD) == 0 {
            // debug!("tx desc status: {}", self.status.read());
        } 
    }
}

impl fmt::Debug for LegacyTxDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{{addr: {:#X}, length: {}, cso: {}, cmd: {}, status: {}, css: {}, special: {}}}",
                    self.phys_addr.read(), self.length.read(), self.cso.read(), self.cmd.read(), self.status.read(), self.css.read(), self.vlan.read())
    }
}



/// This struct is a Legacy Receive Descriptor. 
/// The driver writes to the upper 64 bits, and the NIC writes to the lower 64 bits.
/// It's the descriptor type used in older Intel NICs and the E1000 driver.
#[derive(FromBytes)]
#[repr(C)]
pub struct LegacyRxDescriptor {
    /// The starting physical address of the receive buffer
    pub phys_addr:  Volatile<u64>,      
    /// Length of the receive buffer in bytes
    pub length:     ReadOnly<u16>,
    /// Checksum value of the packet after the IP header till the end 
    pub checksum:   ReadOnly<u16>,
    /// Status bits which tell if the descriptor has been used
    pub status:     Volatile<u8>,
    /// Receive errors
    pub errors:     ReadOnly<u8>,
    /// Vlan tags
    pub vlan:       ReadOnly<u16>,
}

impl RxDescriptor for LegacyRxDescriptor {
    fn init(&mut self, packet_buffer_address: PhysicalAddress) {
        self.phys_addr.write(packet_buffer_address.value() as u64);
        self.status.write(0);
    }

    fn set_packet_address(&mut self, packet_buffer_address: PhysicalAddress) {
        self.phys_addr.write(packet_buffer_address.value() as u64);
    }

    fn reset_status(&mut self) {
        self.status.write(0);
    }

    fn descriptor_done(&self) -> bool {
        (self.status.read() & RX_STATUS_DD) == RX_STATUS_DD
    }

    fn end_of_packet(&self) -> bool {
        (self.status.read() & RX_STATUS_EOP) == RX_STATUS_EOP        
    }

    fn length(&self) -> u64 {
        self.length.read() as u64
    }
}

use core::fmt;
impl fmt::Debug for LegacyRxDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{{addr: {:#X}, length: {}, checksum: {}, status: {}, errors: {}, special: {}}}",
                    self.phys_addr.read(), self.length.read(), self.checksum.read(), self.status.read(), self.errors.read(), self.vlan.read())
    }
}


/// Advanced Receive Descriptor used in the Ixgbe driver.
/// It has 2 modes: Read and Write Back, both of which use the whole 128 bits. 
/// There is one receive descriptor per receive buffer that can be converted between these 2 modes.
/// Read contains the addresses that the driver writes.
/// Write Back contains information the hardware writes on receiving a packet.
/// More information can be found in the 82599 datasheet.
#[derive(FromBytes)]
#[repr(C)]
pub struct AdvancedRxDescriptor {
    /// Starting physcal address of the receive buffer for the packet.
    pub packet_buffer_address:  Volatile<u64>,
    /// Starting physcal address of the receive buffer for the header.
    /// This field will only be used if header splitting is enabled. 
    pub header_buffer_address:  Volatile<u64>,
}

impl RxDescriptor for AdvancedRxDescriptor {
    fn init (&mut self, packet_buffer_address: PhysicalAddress) {
        self.packet_buffer_address.write(packet_buffer_address.value() as u64);
        // set the header address to 0 because packet splitting is not supposed to be enabled in the 82599
        self.header_buffer_address.write(0);
    }

    fn set_packet_address(&mut self, packet_buffer_address: PhysicalAddress) {
        self.packet_buffer_address.write(packet_buffer_address.value() as u64);
    }

    fn reset_status(&mut self) {
        self.header_buffer_address.write(0);
    }

    fn descriptor_done(&self) -> bool{
        (self.get_ext_status() & RX_STATUS_DD as u64) == RX_STATUS_DD as u64
    }

    fn end_of_packet(&self) -> bool {
        (self.get_ext_status() & RX_STATUS_EOP as u64) == RX_STATUS_EOP as u64        
    }

    fn length(&self) -> u64 {
        self.get_pkt_len()
    }
}

impl AdvancedRxDescriptor {

    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Returns the packet type that was used for the Receive Side Scaling hash function.
    pub fn get_rss_type(&self) -> u64{
        self.packet_buffer_address.read().get_bits(0..3) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Returns the packet type as identified by the hardware.
    pub fn get_packet_type(&self) -> u64{
        self.packet_buffer_address.read().get_bits(4..16) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Returns the number of Receive Side Coalesced packets that start in this descriptor.
    pub fn get_rsccnt(&self) -> u64{
        self.packet_buffer_address.read().get_bits(17..20) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Returns the size of the packet header in bytes.
    pub fn get_hdr_len(&self) -> u64{
        self.packet_buffer_address.read().get_bits(21..30) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor.
    /// When set to 1b, indicates that the hardware has found the length of the header.
    pub fn get_sph(&self) -> bool{
        self.packet_buffer_address.read().get_bit(31) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Returns the Receive Side Scaling hash.
    pub fn get_rss_hash(&self) -> u64{
        self.packet_buffer_address.read().get_bits(32..63) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Returns the Flow Director Filter ID if the packet matches a filter.
    pub fn get_fdf_id(&self) -> u64{
        self.packet_buffer_address.read().get_bits(32..63) 
    }

    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Status information indicates whether a descriptor has been used 
    /// and whether the buffer is the last one for a packet
    pub fn get_ext_status(&self) -> u64{
        self.header_buffer_address.read().get_bits(0..19) 
    }
    
    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Returns errors reported by hardware for different packet types
    pub fn get_ext_error(&self) -> u64{
        self.header_buffer_address.read().get_bits(20..31) 
    }
    
    /// Write Back mode function for the Advanced Receive Descriptor.
    /// Returns the number of bytes posted to the packet buffer
    pub fn get_pkt_len(&self) -> u64{
        self.header_buffer_address.read().get_bits(32..47) 
    }
    
    /// Write Back mode function for the Advanced Receive Descriptor.
    /// If the vlan header is stripped from the packet, then the 16 bits of the VLAN tag are posted here
    pub fn get_vlan_tag(&self) -> u64{
        self.header_buffer_address.read().get_bits(48..63) 
    }    
}

impl fmt::Debug for AdvancedRxDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{{Packet buffer address: {:#X}, Packet header address: {:#X}}}",
            self.packet_buffer_address.read(), self.header_buffer_address.read())
    }
}


/// Advanced Transmit Descriptor used by the `ixgbe` NIC driver.
///
/// # Two usage modes
/// It has 2 modes: Read and Write Back, both of which use the whole 128 bits. 
/// There is one transmit descriptor per transmit buffer; it can be converted between these 2 modes.
///
/// Read contains the addresses that the driver writes.
/// Write Back contains information the hardware writes on receiving a packet.
///
/// More information can be found in the 82599 datasheet.
#[derive(FromBytes)]
#[repr(C)]
pub struct AdvancedTxDescriptor {
    /// Starting physical address of the receive buffer for the packet.
    pub packet_buffer_address:  Volatile<u64>,
    /// Length of data buffer
    pub data_len: Volatile<u16>,
    /// A multi-part field:
    /// * `dtyp`: Descriptor Type, occupies bits `[7:4]`,
    /// * `mac`: options to apply LinkSec and time stamp, occupies bits `[3:2]`.
    pub dtyp_mac_rsv : Volatile<u8>,
    /// Command bits
    pub dcmd:  Volatile<u8>,
    /// A multi-part field:
    /// * `paylen`: the size in bytes of the data buffer in host memory.
    ///   not including the fields that the hardware adds), occupies bits `[31:14]`.
    /// * `popts`: options to offload checksum calculation, occupies bits `[13:8]`.
    /// * `sta`: status of the descriptor (whether it's in use or not), occupies bits `[3:0]`.
    pub paylen_popts_cc_idx_sta: Volatile<u32>,
}

impl TxDescriptor for AdvancedTxDescriptor {
    fn init(&mut self) {
        self.packet_buffer_address.write(0);
        self.paylen_popts_cc_idx_sta.write(0);
        self.dcmd.write(0);
        self.dtyp_mac_rsv.write(0);
        self.data_len.write(0);
    }

    fn send(&mut self, transmit_buffer_addr: PhysicalAddress, transmit_buffer_length: u16) {
        self.packet_buffer_address.write(transmit_buffer_addr.value() as u64);
        self.data_len.write(transmit_buffer_length);
        self.dtyp_mac_rsv.write(TX_DTYP_ADV);
        self.paylen_popts_cc_idx_sta.write((transmit_buffer_length as u32) << TX_PAYLEN_SHIFT);
        self.dcmd.write(TX_CMD_DEXT | TX_CMD_RS | TX_CMD_IFCS | TX_CMD_EOP);
    }

    fn wait_for_packet_tx(&self) {
        while (self.paylen_popts_cc_idx_sta.read() as u8 & TX_STATUS_DD) == 0 {
            // error!("tx desc status: {:#X}", self.paylen_popts_cc_idx_sta.read());
        } 
    }
}

impl fmt::Debug for AdvancedTxDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("AdvancedTxDescriptor")
            .field("packet buffer address", &self.packet_buffer_address.read())
            .field("data len", &self.data_len.read())
            .field("dtype_mac_rsv", &self.dtyp_mac_rsv.read())
            .field("dcmd", &self.dcmd.read())
            .field("paylen_popts_cc_idx_sta", &self.paylen_popts_cc_idx_sta.read())
            .finish()
    }
}

