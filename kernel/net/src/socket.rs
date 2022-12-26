use core::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};
use smoltcp::socket::AnySocket;

#[repr(transparent)]
pub struct Socket<T>
where
    T: AnySocket<'static> + ?Sized,
{
    pub(crate) inner: Option<smoltcp::socket::Socket<'static>>,
    phantom_data: PhantomData<T>,
}

impl<T> From<T> for Socket<T>
where
    T: AnySocket<'static>,
{
    fn from(value: T) -> Self {
        Self {
            inner: Some(value.upcast()),
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
        let inner = self.inner.as_ref().unwrap();
        // The only way to create a socket is to upcast a T, so downcasting it cannot
        // fail.
        T::downcast(inner).unwrap()
    }
}

impl<T> DerefMut for Socket<T>
where
    T: AnySocket<'static>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        let inner = self.inner.as_mut().unwrap();
        // The only way to create a socket is to upcast a T, so downcasting it cannot
        // fail.
        T::downcast_mut(inner).unwrap()
    }
}
