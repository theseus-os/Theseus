use super::*;

pub mod ehci;

pub enum PciInterface {
    Ehci(&'static PciDevice),
}

pub(crate) enum Controller {
    Ehci(ehci::EhciController),
}

pub(crate) trait ControllerApi {
    fn common_alloc_mut(&mut self) -> Result<&mut CommonUsbAlloc, &'static str>;
    fn probe_ports(&mut self) -> Result<(), &'static str>;

    fn request(
        &mut self,
        dev_addr: DeviceAddress,
        request: Request,
        max_packet_size: MaxPacketSize,
    ) -> Result<(), &'static str>;

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
