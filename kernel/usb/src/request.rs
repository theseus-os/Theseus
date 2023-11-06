use super::*;

pub enum Request<'a> {
    GetStatus(Target, &'a mut DeviceStatus),

    ClearFeature(Target, FeatureId),
    SetFeature(Target, FeatureId),

    SetAddress(DeviceAddress),

    GetDeviceDescriptor(&'a mut descriptors::Device),
    SetDeviceDescriptor(descriptors::Device),

    GetConfigDescriptor(DescriptorIndex, &'a mut descriptors::Configuration),
    SetConfigDescriptor(DescriptorIndex, descriptors::Configuration),

    GetConfiguration(&'a mut Option<NonZeroU8>),
    SetConfiguration(Option<NonZeroU8>),

    GetInterfaceAltSetting(InterfaceIndex, &'a mut u8),
    SetInterfaceAltSetting(InterfaceIndex, u8),

    ReadString(StringIndex, &'a mut String),

    HidGetReport(InterfaceIndex, HidReportType, HidReportId, &'a mut [u8]),
    HidSetReport(InterfaceIndex, HidReportType, HidReportId, &'a [u8]),
    HidGetProtocol(InterfaceIndex, &'a mut HidProtocol),
    HidSetProtocol(InterfaceIndex, HidProtocol),

    // not supported by this driver
    // SynchFrame(EndpointAddress, u16),
}

pub type HidReportId = u8;

#[bitsize(8)]
#[derive(Copy, Clone, Debug, FromBits)]
pub enum HidReportType {
    Input = 1,
    Output = 2,
    Feature = 3,
    #[fallback]
    Reserved = 0xff,
}

#[bitsize(8)]
#[derive(Copy, Clone, Debug, FromBits)]
pub enum HidProtocol {
    Boot = 0,
    Report = 1,
    #[fallback]
    Reserved = 0xff,
}

impl<'a> Request<'a> {
    pub(crate) fn get_raw(&self) -> RawRequest {
        match self {
            Self::GetStatus(target, _dev_status) => RawRequest::new(
                (*target).into(),
                RequestType::Standard,
                Direction::In,
                std_req::GET_STATUS,
                0u16,
                target.index(),
                2u16,
            ),
            Self::ClearFeature(target, feature_id) => RawRequest::new(
                (*target).into(),
                RequestType::Standard,
                Direction::Out,
                std_req::CLEAR_FEATURE,
                *feature_id,
                target.index(),
                0u16,
            ),
            Self::SetFeature(target, feature_id) => RawRequest::new(
                (*target).into(),
                RequestType::Standard,
                Direction::Out,
                std_req::SET_FEATURE,
                *feature_id,
                target.index(),
                0u16,
            ),
            Self::SetAddress(dev_addr) => RawRequest::new(
                RawRequestRecipient::Device,
                RequestType::Standard,
                Direction::Out,
                std_req::SET_ADDRESS,
                *dev_addr as u16,
                0u16,
                0u16,
            ),
            Self::GetDeviceDescriptor(_descriptor) => RawRequest::new(
                RawRequestRecipient::Device,
                RequestType::Standard,
                Direction::In,
                std_req::GET_DESCRIPTOR,
                (u8::from(DescriptorType::Device) as u16) << 8,
                0u16,
                size_of::<descriptors::Device>() as u16,
            ),
            Self::SetDeviceDescriptor(_descriptor) => RawRequest::new(
                RawRequestRecipient::Device,
                RequestType::Standard,
                Direction::Out,
                std_req::SET_DESCRIPTOR,
                (u8::from(DescriptorType::Device) as u16) << 8,
                0u16,
                size_of::<descriptors::Device>() as u16,
            ),
            Self::GetConfigDescriptor(desc_index, _descriptor) => RawRequest::new(
                RawRequestRecipient::Device,
                RequestType::Standard,
                Direction::In,
                std_req::GET_DESCRIPTOR,
                ((u8::from(DescriptorType::Configuration) as u16) << 8) | (*desc_index as u16),
                0u16,
                size_of::<descriptors::ConfigInner>() as u16,
            ),
            Self::SetConfigDescriptor(desc_index, descriptor) => RawRequest::new(
                RawRequestRecipient::Device,
                RequestType::Standard,
                Direction::Out,
                std_req::SET_DESCRIPTOR,
                ((u8::from(DescriptorType::Configuration) as u16) << 8) | (*desc_index as u16),
                0u16,
                descriptor.inner.total_length,
            ),
            Self::GetConfiguration(_config) => RawRequest::new(
                RawRequestRecipient::Device,
                RequestType::Standard,
                Direction::In,
                std_req::GET_CONFIGURATION,
                0u16,
                0u16,
                1u16,
            ),
            Self::SetConfiguration(config) => RawRequest::new(
                RawRequestRecipient::Device,
                RequestType::Standard,
                Direction::Out,
                std_req::SET_CONFIGURATION,
                config.map(|v| v.into()).unwrap_or(0) as u16,
                0u16,
                0u16,
            ),
            Self::GetInterfaceAltSetting(interface_idx, _alt_setting) => RawRequest::new(
                RawRequestRecipient::Interface,
                RequestType::Standard,
                Direction::In,
                std_req::GET_INTERFACE_ALT_SETTING,
                0u16,
                *interface_idx as u16,
                1u16,
            ),
            Self::SetInterfaceAltSetting(interface_idx, alt_setting) => RawRequest::new(
                RawRequestRecipient::Interface,
                RequestType::Standard,
                Direction::Out,
                std_req::SET_INTERFACE_ALT_SETTING,
                *alt_setting as u16,
                *interface_idx as u16,
                0u16,
            ),
            Self::ReadString(string_idx, _string) => RawRequest::new(
                RawRequestRecipient::Device,
                RequestType::Standard,
                Direction::In,
                std_req::GET_DESCRIPTOR,
                ((u8::from(DescriptorType::String) as u16) << 8) | (*string_idx as u16),
                0u16,
                2u16,
            ),

            // HID REQUESTS

            Self::HidGetReport(int_idx, report_type, report_id, report) => RawRequest::new(
                RawRequestRecipient::Interface,
                RequestType::Class,
                Direction::In,
                hid_req::GET_REPORT,
                ((u8::from(*report_type) as u16) << 8) | (*report_id as u16),
                *int_idx as u16,
                report.len() as u16,
            ),
            Self::HidSetReport(int_idx, report_type, report_id, report) => RawRequest::new(
                RawRequestRecipient::Interface,
                RequestType::Class,
                Direction::Out,
                hid_req::SET_REPORT,
                ((u8::from(*report_type) as u16) << 8) | (*report_id as u16),
                *int_idx as u16,
                report.len() as u16,
            ),
            Self::HidGetProtocol(int_idx, _protocol) => RawRequest::new(
                RawRequestRecipient::Interface,
                RequestType::Class,
                Direction::In,
                hid_req::GET_PROTOCOL,
                0,
                *int_idx as u16,
                1,
            ),
            Self::HidSetProtocol(int_idx, protocol) => RawRequest::new(
                RawRequestRecipient::Interface,
                RequestType::Class,
                Direction::Out,
                hid_req::SET_PROTOCOL,
                u8::from(*protocol) as u16,
                *int_idx as u16,
                0,
            ),
        }
    }

    pub(crate) fn allocate_payload(&self, shmem: &mut CommonUsbAlloc) -> Result<(AllocSlot, UsbPointer), &'static str> {
        match self {
            // STD
            Request::GetStatus(_target, _dev_status) => shmem.words.allocate(None),
            Request::ClearFeature(_target, _feature_id) => Ok(invalid_ptr_slot()),
            Request::SetFeature(_target, _feature_id) => Ok(invalid_ptr_slot()),
            Request::SetAddress(_dev_addr) => Ok(invalid_ptr_slot()),
            Request::GetDeviceDescriptor(_d) => shmem.descriptors.device.allocate(None),
            Request::SetDeviceDescriptor(d) => shmem.descriptors.device.allocate(Some(*d)),
            Request::GetConfigDescriptor(_desc_idx, _d) => shmem.descriptors.configuration.allocate(None),
            Request::SetConfigDescriptor(_desc_idx, d) => shmem.descriptors.configuration.allocate(Some(*d)),
            Request::GetConfiguration(_config) => shmem.bytes.allocate(None),
            Request::SetConfiguration(_config) => Ok(invalid_ptr_slot()),
            Request::GetInterfaceAltSetting(_interface_idx, _alt_setting) => shmem.bytes.allocate(None),
            Request::SetInterfaceAltSetting(_interface_idx, _alt_setting) => Ok(invalid_ptr_slot()),
            Request::ReadString(_string_idx, _string) => shmem.descriptors.string.allocate(None),

            // HID
            Self::HidGetReport(_int_idx, _report_type, _report_id, _report) => shmem.pages.allocate(None),
            Self::HidSetReport(_int_idx, _report_type, _report_id, report) => {
                let (i, addr) = shmem.pages.allocate(None)?;
                let page = shmem.pages.get_mut(i)?;
                page[..report.len()].copy_from_slice(report);
                Ok((i, addr))
            },
            Self::HidGetProtocol(_int_idx, _protocol) => shmem.bytes.allocate(None),
            Self::HidSetProtocol(_int_idx, _protocol) => Ok(invalid_ptr_slot()),
        }
    }

    pub(crate) fn adjust_len(&self, shmem: &CommonUsbAlloc, shmem_index: AllocSlot) -> Result<Option<u16>, &'static str> {
        match self {
            Request::ReadString(_string_idx, _string) => {
                let string_desc = &shmem.descriptors.string.get(shmem_index)?;
                Ok(Some(string_desc.length as u16))
            },
            Request::GetConfigDescriptor(_desc_idx, _desc) => {
                let desc = &shmem.descriptors.configuration.get(shmem_index)?;
                Ok(Some(desc.inner.total_length))
            },
            _ => Ok(None),
        }
    }

    pub(crate) fn free_and_move_payload(self, shmem: &mut CommonUsbAlloc, shmem_index: AllocSlot) -> Result<(), &'static str> {
        match self {
            // STD
            Request::GetStatus(_target, status) => shmem.words.free(shmem_index).map(|word| *status = word.into()),
            Request::ClearFeature(_target, _feature_id) => Ok(()),
            Request::SetFeature(_target, _feature_id) => Ok(()),
            Request::SetAddress(_dev_addr) => Ok(()),
            Request::GetDeviceDescriptor(d) => shmem.descriptors.device.free(shmem_index).map(|desc| *d = desc),
            Request::SetDeviceDescriptor(_descriptor) => shmem.descriptors.device.free(shmem_index).map(|_| ()),
            Request::GetConfigDescriptor(_desc_idx, d) => shmem.descriptors.configuration.free(shmem_index).map(|desc| *d = desc),
            Request::SetConfigDescriptor(_desc_idx, _descriptor) => shmem.descriptors.configuration.free(shmem_index).map(|_| ()),
            Request::GetConfiguration(config) => shmem.bytes.free(shmem_index).map(|byte| *config = NonZeroU8::new(byte)),
            Request::SetConfiguration(_config) => Ok(()),
            Request::GetInterfaceAltSetting(_interface_idx, alt_setting) => shmem.bytes.free(shmem_index).map(|byte| *alt_setting = byte),
            Request::SetInterfaceAltSetting(_interface_idx, _alt_setting) => Ok(()),
            Request::ReadString(_string_idx, string) => {
                let string_desc = &shmem.descriptors.string.free(shmem_index)?;
                let str_len = string_desc.length.checked_sub(2).unwrap_or(0) as usize;
                let slice = &string_desc.unicode_bytes[..str_len];

                string.clear();
                string.push_str(from_utf8(slice).map_err(|_e| "Invalid String Bytes in USB device")?);

                Ok(())
            },

            // HID
            Self::HidGetReport(_int_idx, _report_type, _report_id, report) => {
                let page = shmem.pages.free(shmem_index)?;
                report.copy_from_slice(&page[..report.len()]);
                Ok(())
            },
            Self::HidSetReport(_int_idx, _report_type, _report_id, _report) => shmem.pages.free(shmem_index).map(|_| ()),
            Self::HidGetProtocol(_int_idx, protocol) => shmem.bytes.free(shmem_index).map(|byte| *protocol = byte.into()),
            Self::HidSetProtocol(_int_idx, _protocol) => Ok(()),
        }
    }
}
