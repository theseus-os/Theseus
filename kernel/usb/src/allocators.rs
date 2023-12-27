#![allow(dead_code)]
//! Allocators for structures shared with the USB controller
//!
//! The USB controller is able to access memory at relatively
//! arbitrary addresses. This module provides the crate with
//! allocators via the `allocator!()` macro:
//! ```rust
//! // creates an allocator "RequestAlloc" with 16 slots of RawRequest objects.
//! allocator!(pub(crate) RequestAlloc, RawRequest, 16);
//! ```
//! 
//! Each slot can be designated by either its index or its raw address.
//!
//! This module also creates allocators for common USB structures:
//!
//! | Allocator Name                     | Object                         | slots |
//! |------------------------------------|--------------------------------|-------|
//! | `RequestAlloc`                     | `RawRequest`                   |  16   |
//! | `ByteAlloc`                        | `u8`                           |  16   |
//! | `WordAlloc`                        | `u16`                          |  16   |
//! | `Buf8Alloc`                        | `[u8; 8]`                      |  16   |
//! | `PageAlloc`                        | `[u8; 0x1000]`                 |  4    |
//! | `DeviceDescAlloc`                  | `descriptors::Device`          |  16   |
//! | `ConfigurationDescAlloc`           | `descriptors::Configuration`   |  8    |
//! | `InterfaceDescAlloc`               | `descriptors::Interface`       |  16   |
//! | `EndpointDescAlloc`                | `descriptors::Endpoint`        |  16   |
//! | `DeviceQualifierDescAlloc`         | `descriptors::DeviceQualifier` |  16   |
//! | `OtherSpeedConfigurationDescAlloc` | `descriptors::Configuration`   |  8    |
//! | `StringDescAlloc`                  | `descriptors::UsbString`       |  4    |
///
/// This is an arbitrary module design, not following any specification.

use super::*;

allocator!(pub(crate) RequestAlloc, RawRequest, 16);
allocator!(pub(crate) ByteAlloc, u8, 16);
allocator!(pub(crate) WordAlloc, u16, 16);
allocator!(pub(crate) Buf8Alloc, [u8; 8], 16);
allocator!(pub(crate) PageAlloc, [u8; 0x1000], 4);

allocator!(pub(crate) DeviceDescAlloc, descriptors::Device, 16);
allocator!(pub(crate) ConfigurationDescAlloc, descriptors::Configuration, 8);
allocator!(pub(crate) InterfaceDescAlloc, descriptors::Interface, 16);
allocator!(pub(crate) EndpointDescAlloc, descriptors::Endpoint, 16);
allocator!(pub(crate) DeviceQualifierDescAlloc, descriptors::DeviceQualifier, 16);
allocator!(pub(crate) OtherSpeedConfigurationDescAlloc, descriptors::Configuration, 8);
allocator!(pub(crate) StringDescAlloc, descriptors::UsbString, 4);

#[derive(Debug, FromBytes)]
pub(crate) struct DescriptorAlloc {
    pub device: DeviceDescAlloc,
    pub configuration: ConfigurationDescAlloc,
    pub interface: InterfaceDescAlloc,
    pub endpoint: EndpointDescAlloc,
    pub device_qualifier: DeviceQualifierDescAlloc,
    pub other_speed_configuration: OtherSpeedConfigurationDescAlloc,
    pub string: StringDescAlloc,
}

#[derive(Debug, FromBytes)]
pub(crate) struct CommonUsbAlloc {
    pub descriptors: DescriptorAlloc,
    pub requests: RequestAlloc,
    pub bytes: ByteAlloc,
    pub words: WordAlloc,
    pub buf8: Buf8Alloc,
    pub pages: PageAlloc,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub(crate) struct AllocSlot(pub(crate) usize, pub(crate) TypeId);

impl AllocSlot {
    pub fn check<T: 'static>(&self) -> Result<(), &'static str> {
        match TypeId::of::<T>() == self.1 {
            true => Ok(()),
            false => Err("Invalid AllocSlot"),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub(crate) struct UsbPointer(pub u32);

impl UsbPointer {
    pub(crate) fn from_ref<T>(t_ref: &T) -> Self {
        // todo: check that it's in range
        Self(t_ref as *const T as usize as u32)
    }
}

pub(crate) fn invalid_ptr_slot() -> (AllocSlot, UsbPointer) {
    (AllocSlot(0, TypeId::of::<TypeId>()), UsbPointer(0))
}

#[macro_export]
macro_rules! allocator {
    ($name:ident, $ty:ty, $cap:literal) => {
        allocator!(pub(self) $name, $ty, $cap);
    };
    ($vis:vis $name:ident, $ty:ty, $cap:literal) => {

        #[derive(Debug, FromBytes)]
        $vis struct $name {
            slots: [$ty; $cap],
            occupied: [u8; $cap],
        }

        impl $name {
            const OCCUPIED_TRUE: u8 = 1;
            const OCCUPIED_FALSE: u8 = 0;

            pub fn init(&mut self) {
                self.occupied.fill(Self::OCCUPIED_FALSE);
            }

            pub fn allocate(&mut self, init_to: Option<$ty>) -> Result<(AllocSlot, allocators::UsbPointer), &'static str> {
                for i in 0..$cap {
                    if self.occupied[i] == Self::OCCUPIED_FALSE {
                        self.occupied[i] = Self::OCCUPIED_TRUE;
                        let mut_ref = &mut self.slots[i];
                        if let Some(value) = init_to {
                            *mut_ref = value;
                        }
                        let addr = allocators::UsbPointer::from_ref(mut_ref);
                        let slot = allocators::AllocSlot(i, TypeId::of::<$ty>());

                        // log::warn!("{}: alloc ({})", stringify!($ty), i);
                        return Ok((slot, addr))
                    }
                }

                Err(concat!(stringify!($name), ": Out of slots"))
            }

            pub fn free(&mut self, slot: AllocSlot) -> Result<$ty, &'static str> {
                slot.check::<$ty>()?;
                let err_msg = concat!(stringify!($name), ": Invalid slot key");
                let occupied = self.occupied.get_mut(slot.0).ok_or(err_msg)?;
                *occupied = Self::OCCUPIED_FALSE;
                // log::warn!("{}: free ({})", stringify!($ty), slot.0);
                Ok(self.slots.get(slot.0).unwrap().clone())
            }

            pub fn free_by_addr(&mut self, addr: UsbPointer) -> Result<$ty, &'static str> {
                self.free(self.find(addr)?)
            }

            pub fn get(&self, slot: AllocSlot) -> Result<&$ty, &'static str> {
                slot.check::<$ty>()?;
                let err_msg = concat!(stringify!($name), ": Invalid slot key");
                self.slots.get(slot.0).ok_or(err_msg)
            }

            pub fn get_mut(&mut self, slot: AllocSlot) -> Result<&mut $ty, &'static str> {
                slot.check::<$ty>()?;
                let err_msg = concat!(stringify!($name), ": Invalid slot key");
                self.slots.get_mut(slot.0).ok_or(err_msg)
            }

            pub fn find(&self, addr: UsbPointer) -> Result<AllocSlot, &'static str> {
                let err_msg = concat!(stringify!($name), ": Invalid address");
                let mut_ref = &self.slots[0];
                let addr_of_first = UsbPointer::from_ref(mut_ref);

                let offset = addr.0.checked_sub(addr_of_first.0).ok_or(err_msg)? as usize;

                let type_size = core::mem::size_of::<$ty>();
                let key = (offset / type_size);

                let valid_offset = offset % type_size == 0;
                let valid_key = key < $cap;

                match (valid_offset, valid_key) {
                    (true, true) => Ok(AllocSlot(key, TypeId::of::<$ty>())),
                    _ => Err(err_msg),
                }
            }

            pub fn get_by_addr(&self, addr: UsbPointer) -> Result<&$ty, &'static str> {
                self.get(self.find(addr)?)
            }

            pub fn get_mut_by_addr(&mut self, addr: UsbPointer) -> Result<&mut $ty, &'static str> {
                self.get_mut(self.find(addr)?)
            }

            pub fn address_of(&self, slot: AllocSlot) -> Result<UsbPointer, &'static str> {
                slot.check::<$ty>()?;
                let err_msg = concat!(stringify!($name), ": Invalid slot key");
                Ok(UsbPointer::from_ref(self.slots.get(slot.0).ok_or(err_msg)?))
            }
        }

    };
}
