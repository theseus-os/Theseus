use crate::NetworkInterface;
use alloc::sync::Arc;
use core::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};
use mutex_sleep::MutexSleepGuard;
use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::AnySocket,
    wire::{IpEndpoint, IpListenEndpoint},
};

pub use smoltcp::socket::tcp::ConnectError;

/// A network socket.
///
/// In order to use the socket, it must be locked using the [`lock`] method.
/// This will lock the interface's list of sockets, and so the guard returned by
/// [`lock`] must be dropped before calling [`Interface::poll`].
pub struct Socket<T>
where
    // TODO: Relax 'static lifetime.
    T: AnySocket<'static> + ?Sized,
{
    pub(crate) handle: SocketHandle,
    pub(crate) interface: Arc<NetworkInterface>,
    pub(crate) phantom_data: PhantomData<T>,
}

struct LockedSocket<'a, T>
where
    T: AnySocket<'static> + ?Sized,
{
    handle: SocketHandle,
    sockets: MutexSleepGuard<'a, SocketSet<'static>>,
    interface: &'a Arc<NetworkInterface>,
    phantom_data: PhantomData<T>,
}

impl<'a> LockedSocket<'a, smoltcp::socket::tcp::Socket<'static>> {
    pub fn connect<R, L>(&self, remote_endpoint: R, local_endpoint: L) -> Result<(), ConnectError>
    where
        R: Into<IpEndpoint>,
        L: Into<IpListenEndpoint>,
    {
        (**self).connect(
            self.interface.inner.lock().ctx(),
            remote_endpoint,
            local_endpoint,
        )
    }
}

impl<'a, T> Deref for LockedSocket<'a, T>
where
    T: AnySocket<'static>,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.sockets.get(self.handle)
    }
}

impl<'a, T> DerefMut for LockedSocket<'a, T>
where
    T: AnySocket<'static>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.sockets.get_mut(self.handle)
    }
}

impl<T> Socket<T>
where
    T: AnySocket<'static>,
{
    pub fn lock(&self) -> impl DerefMut<Target = T> + '_ {
        LockedSocket {
            handle: self.handle,
            sockets: self
                .interface
                .sockets
                .lock()
                .expect("failed to lock sockets"),
            interface: &self.interface,
            phantom_data: PhantomData,
        }
    }
}
