//! Defines buffers that are used to send and receive packets.

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate memory;
extern crate mpmc;

use core::ops::{Deref, DerefMut};
use alloc::vec::Vec;
use memory::{PhysicalAddress, MappedPages, PteFlags, create_contiguous_mapping};

/// A buffer that stores a packet to be transmitted through the NIC
/// and is guaranteed to be contiguous in physical memory. 
/// Auto-dereferences into a byte slice that represents its underlying memory. 
pub struct TransmitBuffer {
    mp: MappedPages,
    phys_addr: PhysicalAddress,
    length: u16,
}

impl TransmitBuffer {
    /// Creates a new TransmitBuffer with the specified size in bytes.
    /// The size is a `u16` because that is the maximum size of an NIC transmit buffer. 
    pub fn new(size_in_bytes: u16) -> Result<TransmitBuffer, &'static str> {
        let (mp, starting_phys_addr) = create_contiguous_mapping(
            size_in_bytes as usize,
            PteFlags::new().writable(true).device_memory(true),
        )?;
        Ok(TransmitBuffer {
            mp,
            phys_addr: starting_phys_addr,
            length: size_in_bytes,
        })
    }

    pub fn phys_addr(&self) -> PhysicalAddress {
        self.phys_addr
    }

    pub fn length(&self) -> u16 {
        self.length
    }

    /// Sets the buffers length.
    ///
    /// Returns an error if the length is greater than the current length.
    pub fn set_length(&mut self, length: u16) -> Result<(), &'static str> {
        if length > self.length {
            Err("ReceiveBuffer::set_length(): length too long")
        } else {
            self.length = length;
            Ok(())
        }
    }
}

impl Deref for TransmitBuffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        // We checked that the mapped pages are >= to self.length during initialisation.
        // There can be no overflows since length is a u16, nor can there be alignment
        // issues because we are operating on u8s.
        self.mp.as_slice(0, self.length.into()).unwrap() 
    }
}

impl DerefMut for TransmitBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // We checked that the mapped pages are >= to self.length during initialisation
        // and that they are writable. There can be no overflows since length is
        // a u16, nor can there be alignment issues because we are operating on
        // u8s.
        self.mp.as_slice_mut(0, self.length.into()).unwrap()
    }
}


/// A buffer that stores a packet (a piece of an Ethernet frame) that has been received from the NIC
/// and is guaranteed to be contiguous in physical memory. 
/// Auto-dereferences into a byte slice that represents its underlying memory. 
/// When dropped, its underlying memory is automatically returned to the NIC driver for future reuse.
pub struct ReceiveBuffer {
    mp: MappedPages,
    phys_addr: PhysicalAddress,
    length: u16,
    pool: &'static mpmc::Queue<ReceiveBuffer>,
}

impl ReceiveBuffer {
    /// Creates a new ReceiveBuffer with the given `MappedPages`, `PhysicalAddress`, and `length`. 
    /// When this ReceiveBuffer object is dropped, it will be returned to the given `pool`.
    pub fn new(mp: MappedPages, phys_addr: PhysicalAddress, length: u16, pool: &'static mpmc::Queue<ReceiveBuffer>) -> Result<ReceiveBuffer, &'static str> {
        if usize::from(length) > mp.size_in_bytes() {
            Err("mapped pages too small")
        } else if !mp.flags().is_writable() {
            Err("mapped pages aren't writable")
        } else {
            Ok(ReceiveBuffer {
                mp,
                phys_addr,
                length,
                pool,
            })
        }
    }

    pub fn phys_addr(&self) -> PhysicalAddress {
        self.phys_addr
    }

    pub fn length(&self) -> u16 {
        self.length
    }

    /// Sets the buffers length.
    ///
    /// Returns an error if the length is greater than the current length.
    pub fn set_length(&mut self, length: u16) -> Result<(), &'static str> {
        if length > self.length {
            Err("ReceiveBuffer::set_length(): length too long")
        } else {
            self.length = length;
            Ok(())
        }
    }
}

impl Deref for ReceiveBuffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target{
        // We checked that the mapped pages are >= to self.length during initialisation.
        // There can be no overflows since length is a u16, nor can there be alignment
        // issues because we are operating on u8s.
        self.mp.as_slice(0, usize::from(self.length)).unwrap()
    }
}

impl DerefMut for ReceiveBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // We checked that the mapped pages are >= to self.length during initialisation
        // and that they are writable. There can be no overflows since length is
        // a u16, nor can there be alignment issues because we are operating on
        // u8s.
        self.mp.as_slice_mut(0, usize::from(self.length)).unwrap()
    }
}

impl Drop for ReceiveBuffer {
    fn drop(&mut self) {
        // trace!("ReceiveBuffer::drop(): length: {:5}, phys_addr: {:#X}, vaddr: {:#X}", self.length,  self.phys_addr, self.mp.start_address());

        // We need to return this ReceiveBuffer to its memory pool. We use a clever trick here:
        // Since we cannot move this receive buffer out of `self` because it's borrowed, 
        // we construct a new ReceiveBuffer object that is identical to this one being dropped,
        // and do an in-place replacement of its `MappedPages` object with an empty MP object,
        // allowing us to take ownership of the real MP object and put it into the new_rb. 
        let new_rb = ReceiveBuffer {
            mp: core::mem::replace(&mut self.mp, MappedPages::empty()),
            phys_addr: self.phys_addr,
            length: 0,
            pool: self.pool,
        };
        // we set the length to 0 as a quick way to "clear" the buffer. We could also zero out the whole MP. 

        // Now, we can add the new receive buffer to the pool 
        if let Err(_e) = self.pool.push(new_rb) {
            error!("NIC: couldn't return dropped ReceiveBuffer to pool, buf length: {}, phys_addr: {:#X}", _e.length, _e.phys_addr);
        }

        // `self` will be automatically dropped now, which only has the empty MP object.
    }
}


/// A network (e.g., Ethernet) frame that has been received by the NIC.
pub struct ReceivedFrame(pub Vec<ReceiveBuffer>);
