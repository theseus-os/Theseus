use super::*;

use descriptors::Configuration;
use controllers::ControllerApi;

pub mod hid;

#[derive(Debug)]
pub enum Interface {
    Hid(hid::HidInterface),
}

/// Tells the controller what to do with the interrupt transfer.
#[derive(Debug)]
#[allow(unused)]
pub enum InterruptTransferAction {
    /// This interrupt transfer should be restored back to its initial state.
    Restore,
    /// We're done with this interrupt transfer, it should be destroyed.
    Destroy,
}

pub(crate) fn init(
    controller: &mut dyn ControllerApi,
    device: (DeviceAddress, MaxPacketSize),
    interface: &descriptors::Interface,
    interface_id: InterfaceId,
    config: &Configuration,
    cfg_offset: usize,
) -> Result<Option<Interface>, &'static str> {
    if interface.class == 3 {
        let interface = hid::HidInterface::init(controller, device, interface, interface_id, config, cfg_offset)?;
        Ok(Some(Interface::Hid(interface)))
    } else {
        Ok(None)
    }
}

pub trait InterfaceApi {
    fn on_interrupt_transfer(
        &mut self,
        endpoint: EndpointAddress,
    ) -> Result<InterruptTransferAction, &'static str>;
}
