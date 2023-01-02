use alloc::vec;
use core::any::Any;
use log::error;
use nic_buffers::ReceivedFrame;
use smoltcp::phy;

pub use phy::DeviceCapabilities;

use crate::Error;

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
pub trait NetworkDevice: Send + Sync + Any {
    fn send(&mut self, buf: &[u8]) -> core::result::Result<(), crate::Error>;

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

    fn receive(&mut self) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let frame = self.inner.receive()?;
        Some((RxToken { inner: frame }, TxToken { device: self.inner }))
    }

    fn transmit(&mut self) -> Option<Self::TxToken<'_>> {
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
    fn consume<R, F>(mut self, _timestamp: smoltcp::time::Instant, f: F) -> smoltcp::Result<R>
    where
        F: FnOnce(&mut [u8]) -> smoltcp::Result<R>,
    {
        if self.inner.0.len() > 1 {
            error!(
                "DeviceWrapper::receive(): received frame with {} buffers",
                self.inner.0.len()
            );
        }
        let slice = self.inner.0.first_mut().ok_or(Error::Exhausted)?;
        f(slice)
    }
}

/// The transmit token.
pub(crate) struct TxToken<'a> {
    device: &'a mut dyn NetworkDevice,
}

impl<'a> phy::TxToken for TxToken<'a> {
    fn consume<R, F>(
        self,
        _timestamp: smoltcp::time::Instant,
        len: usize,
        f: F,
    ) -> smoltcp::Result<R>
    where
        F: FnOnce(&mut [u8]) -> smoltcp::Result<R>,
    {
        let mut buf = vec![0; len];
        let ret = f(&mut buf)?;
        self.device.send(&buf)?;
        Ok(ret)
    }
}
