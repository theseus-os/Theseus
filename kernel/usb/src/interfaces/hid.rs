/// HID (boot protocol) interface support

use super::*;

#[derive(Debug, PartialEq)]
enum HidBootProtocol {
    Keyboard,
    Mouse,
    None,
}

/// State of an HID interface
#[derive(Debug, PartialEq)]
pub struct BootHidInterface {
    buf_index: AllocSlot,
    protocol: HidBootProtocol,
}

impl BootHidInterface {
    pub(crate) fn init(
        controller: &mut dyn ControllerApi,
        device: (DeviceAddress, MaxPacketSize),
        interface: &descriptors::Interface,
        interface_id: InterfaceId,
        config: &Configuration,
        cfg_offset: usize,
    ) -> Result<Self, &'static str> {
        let (protocol, data_size) = match interface.protocol {
            0 => Ok((HidBootProtocol::None, 0)),

            // The keyboard protocol sends eight bytes
            // byte 1 is modifier keys status
            // byte 2 is reserved
            // byte 3 to 8 are pressed keys (0 for none, six simultaneous keys max)
            1 => Ok((HidBootProtocol::Keyboard, 8)),

            // The mouse protocol sends three bytes
            // byte 1 contains button status
            // byte 2 contains X displacement
            // byte 3 contains Y displacement
            2 => Ok((HidBootProtocol::Mouse, 3)),

            _ => Err("Invalid Interface Protocol"),
        }?;

        // ensure the boot protocol is currently selected by the device
        controller.request(device.0, Request::HidSetProtocol(interface.interface_number, request::HidProtocol::Boot), device.1)?;

        let buf_index = if protocol != HidBootProtocol::None {
            let alloc = controller.common_alloc_mut()?;
            let (buf_index, buf_addr) = alloc.buf8.allocate(None)?;

            // HID spec, 7.1: the first endpoint descriptor describes the HID Interrupt In endpoint
            let (endpoint, _offset): (&descriptors::Endpoint, _) = config.find_desc(cfg_offset, DescriptorType::Endpoint)?;
            controller.setup_interrupt_transfer(device.0, interface_id, endpoint.address, endpoint.max_packet_size, buf_addr, data_size)?;

            buf_index
        } else {
            log::error!("Unsupported HID Boot Protocol");
            invalid_ptr_slot().0
        };

        Ok(BootHidInterface {
            buf_index,
            protocol,
        })
    }
}

impl InterfaceApi for BootHidInterface {
    fn on_interrupt_transfer(
        &mut self,
        controller: &mut dyn ControllerApi,
        _endpoint: EndpointAddress,
    ) -> Result<InterruptTransferAction, &'static str> {

        // simply logs the event type and uninterpreted payload
        let alloc = controller.common_alloc_mut()?;
        let buf = alloc.buf8.get(self.buf_index)?;
        log::error!("{:?} Event: {:?}", self.protocol, buf);

        Ok(InterruptTransferAction::Restore)
    }
}
