use alloc::{sync::Arc, vec, vec::Vec};
use core::any::Any;
use smoltcp::phy;
use irq_safety::MutexIrqSafe;

pub use phy::DeviceCapabilities;

/// Standard maximum transition unit for ethernet cards.
const STANDARD_MTU: usize = 1500;

pub type DeviceRef = Arc<MutexIrqSafe<dyn crate::Device>>;

/// A network device.
///
/// Devices implementing this trait can then be registered using
/// [`register_device`].
///
/// [`register_device`]: crate::register_device
pub trait Device: Send + Sync + Any {
    fn send(&mut self, buf: &[u8]) -> core::result::Result<(), crate::Error>;

    fn receive(&mut self) -> Option<Vec<u8>>;

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
#[derive(Clone)]
pub(crate) struct DeviceWrapper {
    // pub(crate) inner: Arc<MutexIrqSafe<dyn crate::Device>>,
    pub(crate) inner: &'static MutexIrqSafe<dyn crate::Device>,
}

impl DeviceWrapper {
    pub(crate) fn new<T>(device: T) -> Self
    where
        T: 'static + crate::Device + Send + Sync,
    {
        Self { inner: Arc::new(MutexIrqSafe::new(device)) }
    }
}

impl<'a> phy::Device<'a> for DeviceWrapper {
    type RxToken = RxToken;

    type TxToken = TxToken;

    fn receive(&'a mut self) -> Option<(Self::RxToken, Self::TxToken)> {
        let mut device = self.inner.lock();
        let rx_token = RxToken { inner: device.receive()? };
        Some((rx_token, TxToken { device: self.clone() }))
    }

    fn transmit(&'a mut self) -> Option<Self::TxToken> {
        Some(TxToken { device: self.clone() })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        self.inner.lock().capabilities()
    }
}

/// The receive token.
pub struct RxToken {
    // TODO: Use nic_buffers style memory reuse.
    inner: Vec<u8>,
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
pub struct TxToken {
    device: DeviceWrapper,
}

impl phy::TxToken for TxToken {
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
        self.device.inner.lock().send(&buf)?;
        Ok(ret)
    }
}
