/// Drivers for USB Controllers

use super::*;

pub mod ehci;

/// The USB Controller type
/// 
/// Currently, only EHCI is supported
pub enum ControllerType {
    Ehci,
}

pub(crate) enum Controller {
    Ehci(ehci::EhciController),
}

/// The API implemented by controller drivers
pub(crate) trait ControllerApi {
    /// Returns the allocator for common structures
    fn common_alloc_mut(&mut self) -> Result<&mut CommonUsbAlloc, &'static str>;

    /// Handles any USB port connection/disconnection event
    fn probe_ports(&mut self) -> Result<(), &'static str>;

    /// Sends a request to a device and waits for the device to process it
    fn request(
        &mut self,
        dev_addr: DeviceAddress,
        request: Request,
        max_packet_size: MaxPacketSize,
    ) -> Result<(), &'static str>;

    /// Schedules an interrupt transfer to be polled as frequently as possible
    fn setup_interrupt_transfer(
        &mut self,
        device: DeviceAddress,
        interface: InterfaceId,
        endpoint: EndpointAddress,
        ep_max_packet_size: MaxPacketSize,
        buffer: UsbPointer,
        size: usize,
    ) -> Result<(), &'static str>;

    // todo: bulk transfers

    /// Handles an interrupt signaled to the OS by the controller
    fn handle_interrupt(&mut self) -> Result<(), &'static str>;
}

impl Deref for Controller {
    type Target = dyn ControllerApi;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Ehci(ehci) => ehci,
        }
    }
}

impl DerefMut for Controller {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Ehci(ehci) => ehci,
        }
    }
}
