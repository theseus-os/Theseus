use super::*;

pub enum Descriptor {
    Device(Device),
    Configuration(Configuration),
    Interface(Interface),
    Endpoint(Endpoint),
    DeviceQualifier(DeviceQualifier),
    OtherSpeedConfiguration(Configuration),
}

impl Descriptor {
    pub fn get_length(&self) -> u16 {
        (match self {
            Self::Device(_) => size_of::<Device>(),
            Self::Configuration(_) => size_of::<Configuration>(),
            Self::Interface(_) => size_of::<Interface>(),
            Self::Endpoint(_) => size_of::<Endpoint>(),
            Self::DeviceQualifier(_) => size_of::<DeviceQualifier>(),
            Self::OtherSpeedConfiguration(_) => size_of::<Configuration>(),
        }) as u16
    }

    pub fn get_type(&self) -> u16 {
        u8::from(match self {
            Self::Device(_) => DescriptorType::Device,
            Self::Configuration(_) => DescriptorType::Configuration,
            Self::Interface(_) => DescriptorType::Interface,
            Self::Endpoint(_) => DescriptorType::Endpoint,
            Self::DeviceQualifier(_) => DescriptorType::DeviceQualifier,
            Self::OtherSpeedConfiguration(_) => DescriptorType::Configuration,
        }) as u16
    }
}

#[bitsize(8)]
#[derive(Debug, Copy, Clone, FromBits)]
pub enum DescriptorType {
    Device = 1,
    Configuration = 2,

    String = 3,

    Interface = 4,
    Endpoint = 5,
    DeviceQualifier = 6,

    // Same struct as Self::Configuration
    OtherSpeedConfiguration = 7,

    // Couldn't find the definition for this descriptor
    InterfacePower = 8,

    HumanInputDevice = 33,

    #[fallback]
    Reserved = 0xff,
}

#[derive(Copy, Clone, Debug, FromBytes, Default)]
#[repr(packed)]
pub struct Device {
    pub length: u8,
    pub descriptor_type: u8,
    pub usb_version: u16,
    pub device_class: u8,
    pub device_sub_class: u8,
    pub device_protocol: u8,
    // in this descriptor, this field is a u8
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
#[repr(packed)]
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

#[derive(Copy, Clone, FromBytes, Debug)]
#[repr(packed)]
pub struct Configuration {
    pub inner: ConfigInner,
    // the following are equivalent:
    //
    pub details: [u8; 0x1000],
}

#[derive(Copy, Clone, Debug, FromBytes, Default)]
#[repr(packed)]
pub struct ConfigInner {
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

impl Configuration {
    pub fn find_desc<T: FromBytes>(&self, start_search_at: usize, search: DescriptorType) -> Result<(&T, usize), &'static str> {
        let len = (self.inner.total_length - 9) as usize;
        let mut i = start_search_at;
        while i < len {
            let desc_len = self.details[i] as usize;
            let desc_type = self.details[i + 1];
            if desc_len == 0 {
                return Err("Malformed descriptor");
            }

            if u8::from(search) == desc_type {
                let ptr = (&self.details[i]) as *const u8;
                let cast_ptr: *const T = ptr.cast();
                let maybe_ref = unsafe { cast_ptr.as_ref() };
                return maybe_ref
                    .ok_or("Failed to point to configuration buffer")
                    .map(|t| (t, i + desc_len));
            }
            
            i += desc_len;
        }

        Err("Invalid descriptor index (out of bounds)")
    }
}

#[bitsize(8)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes, Default)]
pub struct ConfigurationAttributes {
    reserved: u5,
    remote_wakeup: bool,
    self_powered: bool,
    reserved: bool,
}

#[derive(Copy, Clone, Debug, FromBytes)]
#[repr(packed)]
pub struct Interface {
    pub length: u8,
    pub descriptor_type: u8,
    pub interface_number: InterfaceIndex,
    pub alt_setting: u8,
    pub num_endpoints: u8,
    pub class: u8,
    pub sub_class: u8,
    pub protocol: u8,
    pub name: StringIndex,
}

#[derive(Copy, Clone, Debug, FromBytes)]
#[repr(packed)]
pub struct Endpoint {
    pub length: u8,
    pub descriptor_type: u8,
    pub address: EndpointAddress,
    pub attributes: EndpointAttributes,
    // in this descriptor, it's a u16
    pub max_packet_size: MaxPacketSize,
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

#[derive(Copy, Clone, Debug, FromBytes)]
#[repr(packed)]
pub struct UsbString {
    pub length: u8,
    pub descriptor_type: u8,
    pub unicode_bytes: [u8; 253],
}
