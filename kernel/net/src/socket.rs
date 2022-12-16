use core::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};
use smoltcp::{iface::SocketSet, socket::AnySocket};

#[repr(transparent)]
pub struct Socket<T>
where
    T: AnySocket<'static> + ?Sized,
{
    pub(crate) inner: SocketSet<'static>,
    phantom_data: PhantomData<T>,
}

impl<T> Socket<T>
where
    T: AnySocket<'static> + ?Sized,
{
    pub(crate) fn new(inner: SocketSet<'static>) -> Self {
        Self {
            inner,
            phantom_data: PhantomData,
        }
    }
}

impl<T> Deref for Socket<T>
where
    T: AnySocket<'static>,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        AnySocket::downcast(self.inner.iter().next().expect("no socket in socket set").1)
            .expect("incorrect socket type")
    }
}

impl<T> DerefMut for Socket<T>
where
    T: AnySocket<'static>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        AnySocket::downcast_mut(
            self.inner
                .iter_mut()
                .next()
                .expect("no socket in socket set")
                .1,
        )
        .expect("incorrect socket type")
    }
}
