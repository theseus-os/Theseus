use super::*;

pub enum Descriptor {
    Device(Device),
    Configuration(Configuration),
    Interface(Interface),
    Endpoint(Endpoint),
    DeviceQualifier(DeviceQualifier),
    OtherSpeedConfiguration(Configuration),
}

#[bitsize(8)]
#[derive(Debug, Copy, Clone, FromBits)]
pub enum DescriptorType {
    Device = 0x1,
    Configuration = 0x2,

    String = 0x3,

    Interface = 0x4,
    Endpoint = 0x5,
    DeviceQualifier = 0x6,

    // Same struct as Self::Configuration
    OtherSpeedConfiguration = 0x7,

    // Couldn't find the definition for this descriptor
    InterfacePower = 0x8,
    #[fallback]
    Reserved = 0xff,
}

#[derive(Copy, Clone, Debug, FromBytes)]
#[repr(C)]
pub struct Device {
    pub length: u8,
    pub descriptor_type: u8,
    pub usb_version: u16,
    pub device_class: u8,
    pub device_sub_class: u8,
    pub device_protocol: u8,
    pub max_packet_size: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    pub device_version: u16,
    pub vendor_str: StringIndex,
    pub product_str: StringIndex,
    pub serial_str: StringIndex,
    pub conf_count: u8,
}

#[derive(Copy, Clone, Debug, FromBytes)]
#[repr(C)]
pub struct DeviceQualifier {
    pub length: u8,
    pub descriptor_type: u8,
    pub usb_version: u16,
    pub class: u8,
    pub sub_class: u8,
    pub protocol: u8,
    pub max_packet_size: u8,
    pub conf_count: u8,
    pub reserved: u8,
}

#[derive(Copy, Clone, Debug, FromBytes)]
#[repr(C)]
pub struct Configuration {
    pub length: u8,
    pub descriptor_type: u8,
    pub total_length: u16,
    pub num_interfaces: u8,
    pub set_config_value: u8,
    pub config_name: StringIndex,
    pub attributes: ConfigurationAttributes,
    /// Expressed in 2mA units (i.e., 50 = 100mA)
    pub max_power: u8,
}

#[bitsize(8)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
pub struct ConfigurationAttributes {
    reserved: u5,
    remote_wakeup: bool,
    self_powered: bool,
    reserved: bool,
}

#[derive(Copy, Clone, Debug, FromBytes)]
#[repr(C)]
pub struct Interface {
    pub length: u8,
    pub descriptor_type: u8,
    pub interface_number: u8,
    pub alt_setting: u8,
    pub num_endpoints: u8,
    pub interface_class: u8,
    pub interface_sub_class: ConfigurationAttributes,
    pub interface_protocol: u8,
    pub interface_name: StringIndex,
}

#[derive(Copy, Clone, Debug, FromBytes)]
#[repr(C)]
pub struct Endpoint {
    pub length: u8,
    pub descriptor_type: u8,
    pub address: EndpointAddress,
    pub attributes: EndpointAttributes,
    pub max_packet_size: u16,
    /// Interval for polling endpoint for data transfers.
    /// Expressed in frames or microframes depending on the device operating
    /// speed (i.e., either 1 millisecond or 125 μs units).
    pub interval: u8,
}

#[bitsize(8)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
pub struct EndpointAttributes {
    transfer_type: EndpointTransferType,
    isoc_endpoint_sync_type: IsochronousEndpointSyncType,
    isoc_endpoint_usage_type: IsochronousEndpointUsageType,
    reserved: u2,
}

#[bitsize(2)]
#[derive(Debug, FromBits)]
pub enum EndpointTransferType {
    Control = 0x0,
    Isochronous = 0x1,
    Bulk = 0x2,
    Interrupt = 0x3,
}

#[bitsize(2)]
#[derive(Debug, FromBits)]
pub enum IsochronousEndpointSyncType {
    None = 0x0,
    Asynchronous = 0x1,
    Adaptive = 0x2,
    Synchronous = 0x3,
}

#[bitsize(2)]
#[derive(Debug, FromBits)]
pub enum IsochronousEndpointUsageType {
    Data = 0x0,
    Feedback = 0x1,
    ImplicitFeedbackData = 0x2,
    Reserved = 0x3,
}
