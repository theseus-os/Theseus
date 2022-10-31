use alloc::{boxed::Box, vec};
use core::any::Any;
use log::error;
use nic_buffers::ReceivedFrame;
use owning_ref::BoxRefMut;
use smoltcp::phy;

pub use phy::DeviceCapabilities;

/// Standard maximum transition unit for ethernet cards.
const STANDARD_MTU: usize = 1500;

/// A network device.
///
/// Devices implementing this trait can then be registered using
/// [`register_device`].
///
/// [`register_device`]: crate::register_device
pub trait Device: Send + Sync + Any {
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
    pub(crate) inner: &'a mut dyn Device,
}

impl<'a, 'b> phy::Device<'a> for DeviceWrapper<'b> {
    type RxToken = RxToken;

    type TxToken = TxToken<'a>;

    fn receive(&'a mut self) -> Option<(Self::RxToken, Self::TxToken)> {
        let frame = self.inner.receive()?;

        if frame.0.len() > 1 {
            error!(
                "DeviceWrapper::receive(): received frame with {} buffers",
                frame.0.len()
            );
        }

        let byte_slice = BoxRefMut::new(Box::new(frame)).map_mut(|frame| frame.0[0].as_slice_mut());

        let rx_token = RxToken { inner: byte_slice };
        Some((rx_token, TxToken { device: self.inner }))
    }

    fn transmit(&'a mut self) -> Option<Self::TxToken> {
        Some(TxToken { device: self.inner })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        self.inner.capabilities()
    }
}

/// The receive token.
pub(crate) struct RxToken {
    inner: BoxRefMut<ReceivedFrame, [u8]>,
}

impl phy::RxToken for RxToken {
    fn consume<R, F>(mut self, _timestamp: smoltcp::time::Instant, f: F) -> smoltcp::Result<R>
    where
        F: FnOnce(&mut [u8]) -> smoltcp::Result<R>,
    {
        f(&mut self.inner)
    }
}

/// The transmit token.
pub(crate) struct TxToken<'a> {
    device: &'a mut dyn Device,
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
