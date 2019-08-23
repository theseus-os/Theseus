//! This crate implements the virtual memory subsystem for Theseus,
//! which is fairly robust and provides a unification between 
//! arbitrarily mapped sections of memory and Rust's lifetime system. 
//! Originally based on Phil Opp's blog_os. 

// Wenqiu: remove features
#![no_std]
#![feature(asm)]
#![feature(ptr_internals)]
#![feature(core_intrinsics)]
#![feature(unboxed_closures)]
#![feature(step_trait, range_is_empty)]

use super::*;

/// A virtual memory address, which is a `usize` under the hood.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
    Debug, Display, Binary, Octal, LowerHex, UpperHex,
    BitAnd, BitOr, BitXor, BitAndAssign, BitOrAssign, BitXorAssign, 
    Add, Sub, AddAssign, SubAssign
)]
#[repr(transparent)]
pub struct VirtualAddress(usize);

impl VirtualAddress {
    /// Creates a new `VirtualAddress`, 
    /// checking that the address is canonical, 
    /// i.e., bits (64:48] are sign-extended from bit 47.
    pub fn new(virt_addr: usize) -> Result<VirtualAddress, &'static str> {
        match virt_addr.get_bits(47..64) {
            0 | 0b1_1111_1111_1111_1111 => Ok(VirtualAddress(virt_addr)),
            _ => Err("VirtualAddress bits 48-63 must be a sign-extension of bit 47"),
        }
    }

    /// Creates a new `VirtualAddress` that is guaranteed to be canonical
    /// by forcing the upper bits (64:48] to be sign-extended from bit 47.
    pub fn new_canonical(mut virt_addr: usize) -> VirtualAddress {
        match virt_addr.get_bit(47) {
            false => virt_addr.set_bits(48..64, 0),
            true  => virt_addr.set_bits(48..64, 0xffff),
        };
        VirtualAddress(virt_addr)
    }

    /// Creates a VirtualAddress with the value 0.
    pub const fn zero() -> VirtualAddress {
        VirtualAddress(0)
    }

    /// Returns the underlying `usize` value for this `VirtualAddress`.
    #[inline]
    pub fn value(&self) -> usize {
        self.0
    }

    /// Returns the offset that this VirtualAddress specifies into its containing memory Page.
    /// 
    /// For example, if the PAGE_SIZE is 4KiB, then this will return 
    /// the least significant 12 bits (12:0] of this VirtualAddress.
    pub fn page_offset(&self) -> usize {
        self.0 & (PAGE_SIZE - 1)
    }
}

impl core::fmt::Pointer for VirtualAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:p}", self.0 as *const usize)
    }
}

impl Add<usize> for VirtualAddress {
    type Output = VirtualAddress;

    fn add(self, rhs: usize) -> VirtualAddress {
        VirtualAddress::new_canonical(self.0.saturating_add(rhs))
    }
}

impl AddAssign<usize> for VirtualAddress {
    fn add_assign(&mut self, rhs: usize) {
        *self = VirtualAddress::new_canonical(self.0.saturating_add(rhs));
    }
}

impl Sub<usize> for VirtualAddress {
    type Output = VirtualAddress;

    fn sub(self, rhs: usize) -> VirtualAddress {
        VirtualAddress::new_canonical(self.0.saturating_sub(rhs))
    }
}

impl SubAssign<usize> for VirtualAddress {
    fn sub_assign(&mut self, rhs: usize) {
        *self = VirtualAddress::new_canonical(self.0.saturating_sub(rhs));
    }
}

impl From<VirtualAddress> for usize {
    #[inline]
    fn from(virt_addr: VirtualAddress) -> usize {
        virt_addr.0
    }
}


/// A physical memory address, which is a `usize` under the hood.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
    Debug, Display, Binary, Octal, LowerHex, UpperHex,
    BitAnd, BitOr, BitXor, BitAndAssign, BitOrAssign, BitXorAssign, 
    Add, Sub, Mul, Div, Rem, Shr, Shl, 
    AddAssign, SubAssign, MulAssign, DivAssign, RemAssign, ShrAssign, ShlAssign
)]
#[repr(transparent)]
pub struct PhysicalAddress(usize);

impl PhysicalAddress {
    /// Creates a new `PhysicalAddress`, 
    /// checking that the bits (64:52] are 0.
    pub fn new(phys_addr: usize) -> Result<PhysicalAddress, &'static str> {
        match phys_addr.get_bits(52..64) {
            0 => Ok(PhysicalAddress(phys_addr)),
            _ => Err("PhysicalAddress bits 52-63 must be zero"),
        }
    }

    /// Creates a new `PhysicalAddress` that is guaranteed to be canonical
    /// by forcing the upper bits (64:52] to be 0.
    pub fn new_canonical(mut phys_addr: usize) -> PhysicalAddress {
        phys_addr.set_bits(52..64, 0);
        PhysicalAddress(phys_addr)
    }

    /// Returns the underlying `usize` value for this `PhysicalAddress`.
    #[inline]
    pub fn value(&self) -> usize {
        self.0
    }

    /// Creates a PhysicalAddress with the value 0.
    pub const fn zero() -> PhysicalAddress {
        PhysicalAddress(0)
    }

    /// Returns the offset that this PhysicalAddress specifies into its containing memory Frame.
    /// 
    /// For example, if the PAGE_SIZE is 4KiB, then this will return 
    /// the least significant 12 bits (12:0] of this PhysicalAddress.
    pub fn frame_offset(&self) -> usize {
        self.0 & (PAGE_SIZE - 1)
    }
}


impl Add<usize> for PhysicalAddress {
    type Output = PhysicalAddress;

    fn add(self, rhs: usize) -> PhysicalAddress {
        PhysicalAddress::new_canonical(self.0.saturating_add(rhs))
    }
}

impl AddAssign<usize> for PhysicalAddress {
    fn add_assign(&mut self, rhs: usize) {
        *self = PhysicalAddress::new_canonical(self.0.saturating_add(rhs));
    }
}

impl Sub<usize> for PhysicalAddress {
    type Output = PhysicalAddress;

    fn sub(self, rhs: usize) -> PhysicalAddress {
        PhysicalAddress::new_canonical(self.0.saturating_sub(rhs))
    }
}

impl SubAssign<usize> for PhysicalAddress {
    fn sub_assign(&mut self, rhs: usize) {
        *self = PhysicalAddress::new_canonical(self.0.saturating_sub(rhs));
    }
}

impl From<PhysicalAddress> for usize {
    #[inline]
    fn from(virt_addr: PhysicalAddress) -> usize {
        virt_addr.0
    }
}