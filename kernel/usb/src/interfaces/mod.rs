use super::*;

use descriptors::Configuration;
use controllers::ControllerApi;

pub mod hid;

#[derive(Debug, PartialEq)]
pub(crate) enum Interface {
    Hid(hid::BootHidInterface),
}

/// Tells the controller what to do with the interrupt transfer.
#[derive(Copy, Clone, Debug, PartialEq)]
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
    if interface.class == 3 && interface.sub_class == 1 {
        let interface = hid::BootHidInterface::init(controller, device, interface, interface_id, config, cfg_offset)?;
        Ok(Some(Interface::Hid(interface)))
    } else {
        Ok(None)
    }
}

pub(crate) trait InterfaceApi {
    fn on_interrupt_transfer(
        &mut self,
        controller: &mut dyn ControllerApi,
        endpoint: EndpointAddress,
    ) -> Result<InterruptTransferAction, &'static str>;
}

impl Deref for Interface {
    type Target = dyn InterfaceApi;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Hid(hid) => hid,
        }
    }
}

impl DerefMut for Interface {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Hid(hid) => hid,
        }
    }
}
