use alloc::vec;

use log::error;
use nic_buffers::{ReceivedFrame, TransmitBuffer};
use smoltcp::phy;
pub use smoltcp::phy::DeviceCapabilities;

/// Standard maximum transition unit for ethernet cards.
const STANDARD_MTU: usize = 1500;

/// A network device.
///
/// Devices implementing this trait can then be registered using
/// [`register_device`].
///
/// Users should not interact with this trait directly, but [`NetworkInterface`]
/// instead.
///
/// [`register_device`]: crate::register_device
/// [`NetworkInterface`]: crate::NetworkInterface.
pub trait NetworkDevice: Send + Sync {
    /// Sends a buffer on the device.
    ///
    /// In the case of an error, the packet is dropped and the error is handled
    /// by the device driver.
    fn send(&mut self, buf: TransmitBuffer);

    /// Receives a frame from the device.
    fn receive(&mut self) -> Option<ReceivedFrame>;

    /// Returns the MAC address.
    fn mac_address(&self) -> [u8; 6];

    /// Returns the device capabilities.
    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = STANDARD_MTU;
        caps
    }
}

/// Wrapper around a network device.
///
/// We use this because we can't directly implement [`phy::Device`] for `T` due
/// to the following error:
/// ```
/// error[E0210]: type parameter `T` must be used as the type parameter for some local type (e.g., `MyStruct<T>`)
/// ```
pub(crate) struct DeviceWrapper<'a> {
    pub(crate) inner: &'a mut dyn NetworkDevice,
}

impl<'a> phy::Device for DeviceWrapper<'a> {
    type RxToken<'b> = RxToken where Self: 'b;

    type TxToken<'c> = TxToken<'c> where Self: 'c;

    fn receive(
        &mut self,
        _: smoltcp::time::Instant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let frame = self.inner.receive()?;
        Some((RxToken { inner: frame }, TxToken { device: self.inner }))
    }

    fn transmit(&mut self, _: smoltcp::time::Instant) -> Option<Self::TxToken<'_>> {
        Some(TxToken { device: self.inner })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        self.inner.capabilities()
    }
}

/// The receive token.
pub(crate) struct RxToken {
    inner: ReceivedFrame,
}

impl phy::RxToken for RxToken {
    fn consume<R, F>(mut self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        if self.inner.0.len() > 1 {
            error!(
                "DeviceWrapper::receive(): received frame with {} buffers",
                self.inner.0.len()
            );
        }
        let slice = self
            .inner
            .0
            .first_mut()
            .expect("received frame spanning no buffers");
        f(slice)
    }
}

/// The transmit token.
pub(crate) struct TxToken<'a> {
    device: &'a mut dyn NetworkDevice,
}

impl<'a> phy::TxToken for TxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        match u16::try_from(len) {
            Ok(len) => {
                // This will only fail if the underlying memory allocation fails.
                let mut buf = TransmitBuffer::new(len).expect("failed to allocate transmit buffer");
                let ret = f(&mut buf);
                self.device.send(buf);
                ret
            }
            Err(_) => {
                // For appropriate behavior on an error, see this smoltcp changelog entry:
                // <https://github.com/smoltcp-rs/smoltcp/blob/fa7fd3c321b8a3bbe1a8a4ee2ee5dc1b63231d6b/CHANGELOG.md?plain=1#L57>
                error!("packet too large: dropping packet");
                let mut buf = vec![0; len];
                f(&mut buf)
            }
        }
    }
}
