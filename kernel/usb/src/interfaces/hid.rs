use super::*;

#[derive(Debug)]
pub struct HidInterface;

impl HidInterface {
    pub(crate) fn init(
        controller: &mut dyn ControllerApi,
        device: (DeviceAddress, MaxPacketSize),
        interface: &descriptors::Interface,
        interface_id: InterfaceId,
        config: &Configuration,
        cfg_offset: usize,
    ) -> Result<Self, &'static str> {
        // todo: read HID descriptor here
        // todo: check it's a keyboard

        let interface_num = interface.interface_number;
        controller.request(device.0, Request::HidSetProtocol(interface_num, request::HidProtocol::Boot), device.1)?;

        let alloc = controller.common_alloc_mut()?;
        let (_buf_index, buf_addr) = alloc.buf8.allocate(None)?;

        let (endpoint, _offset): (&descriptors::Endpoint, _) = config.find_desc(cfg_offset, DescriptorType::Endpoint)?;
        controller.setup_interrupt_transfer(device.0, interface_id, endpoint.address, endpoint.max_packet_size, buf_addr, 8)?;

        Ok(HidInterface)
    }
}

impl InterfaceApi for HidInterface {
    fn on_interrupt_transfer(
        &mut self,
        _endpoint: EndpointAddress,
    ) -> Result<InterruptTransferAction, &'static str> {
        todo!()
    }
}
