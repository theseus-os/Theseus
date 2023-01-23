use core::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};
use mutex_sleep::{MutexSleep, MutexSleepGuard};
use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::AnySocket,
};

/// A network socket.
///
/// In order to use the socket, it must be locked using the [`lock`] method.
/// This will lock the interface's list of sockets, and so the guard returned by
/// [`lock`] must be dropped before calling [`Interface::poll`].
pub struct Socket<'a, T>
where
    T: AnySocket<'static> + ?Sized,
{
    pub(crate) handle: SocketHandle,
    pub(crate) sockets: &'a MutexSleep<SocketSet<'static>>,
    pub(crate) phantom_data: PhantomData<T>,
}

struct LockedSocket<'a, T>
where
    T: AnySocket<'static> + ?Sized,
{
    handle: SocketHandle,
    sockets: MutexSleepGuard<'a, SocketSet<'static>>,
    phantom_data: PhantomData<T>,
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

impl<'a, T> Socket<'a, T>
where
    T: AnySocket<'static> + 'a,
{
    pub fn lock(&self) -> impl DerefMut<Target = T> + 'a {
        LockedSocket {
            handle: self.handle,
            sockets: self.sockets.lock().expect("failed to lock sockets"),
            phantom_data: PhantomData,
        }
    }
}
