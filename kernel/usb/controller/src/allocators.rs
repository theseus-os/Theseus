#![allow(dead_code)]

use super::*;

allocator!(pub(crate) RequestAlloc, RawRequest, 16);
allocator!(pub(crate) ByteAlloc, u8, 16);
allocator!(pub(crate) WordAlloc, u16, 16);
allocator!(pub(crate) PageAlloc, [u8; 0x1000], 4);

allocator!(pub(crate) DeviceDescAlloc, descriptors::Device, 16);
allocator!(pub(crate) ConfigurationDescAlloc, descriptors::Configuration, 8);
allocator!(pub(crate) InterfaceDescAlloc, descriptors::Interface, 16);
allocator!(pub(crate) EndpointDescAlloc, descriptors::Endpoint, 16);
allocator!(pub(crate) DeviceQualifierDescAlloc, descriptors::DeviceQualifier, 16);
allocator!(pub(crate) OtherSpeedConfigurationDescAlloc, descriptors::Configuration, 16);
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
    pub pages: PageAlloc,
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

            pub fn allocate(&mut self, init_to: Option<$ty>) -> Result<(usize, u32), &'static str> {
                for i in 0..$cap {
                    if self.occupied[i] == Self::OCCUPIED_FALSE {
                        self.occupied[i] = Self::OCCUPIED_TRUE;
                        let mut_ref = &mut self.slots[i];
                        if let Some(value) = init_to {
                            *mut_ref = value;
                        }
                        let addr = mut_ref as *mut _ as usize as u32;

                        // log::warn!("{}: alloc ({})", stringify!($ty), i);
                        return Ok((i, addr))
                    }
                }

                Err(concat!(stringify!($name), ": Out of slots"))
            }

            pub fn free(&mut self, index: usize) -> Result<$ty, &'static str> {
                let err_msg = concat!(stringify!($name), ": Invalid slot index");
                let occupied = self.occupied.get_mut(index).ok_or(err_msg)?;
                *occupied = Self::OCCUPIED_FALSE;
                // log::warn!("{}: free ({})", stringify!($ty), index);
                Ok(self.slots.get(index).unwrap().clone())
            }

            pub fn get(&self, index: usize) -> Result<&$ty, &'static str> {
                let err_msg = concat!(stringify!($name), ": Invalid slot index");
                self.slots.get(index).ok_or(err_msg)
            }

            pub fn get_mut(&mut self, index: usize) -> Result<&mut $ty, &'static str> {
                let err_msg = concat!(stringify!($name), ": Invalid slot index");
                self.slots.get_mut(index).ok_or(err_msg)
            }

            pub fn find(&self, addr: u32) -> Result<usize, &'static str> {
                let err_msg = concat!(stringify!($name), ": Invalid address");
                let mut_ref = &self.slots[0];
                let addr_of_first = mut_ref as *const _ as usize as u32;

                let offset = addr.checked_sub(addr_of_first).ok_or(err_msg)? as usize;

                let type_size = core::mem::size_of::<$ty>();
                let index = (offset / type_size);

                let valid_offset = offset % type_size == 0;
                let valid_index = index < $cap;

                match (valid_offset, valid_index) {
                    (true, true) => Ok(index),
                    _ => Err(err_msg),
                }
            }

            pub fn get_by_addr(&self, addr: u32) -> Result<&$ty, &'static str> {
                self.get(self.find(addr)?)
            }

            pub fn get_mut_by_addr(&mut self, addr: u32) -> Result<&mut $ty, &'static str> {
                self.get_mut(self.find(addr)?)
            }

            pub fn address_of(&self, index: usize) -> Result<u32, &'static str> {
                let err_msg = concat!(stringify!($name), ": Invalid slot index");
                let reference = self.slots.get(index).ok_or(err_msg)?;
                Ok(reference as *const _ as usize as u32)
            }
        }

    };
}
