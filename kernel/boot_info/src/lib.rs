//! This crate provides an abstraction over multiboot2 and UEFI boot
//! information.
//!
//! It allows the kernel's initialisation to be the same, regardless of whether
//! it was booted using BIOS or UEFI.

#![feature(type_alias_impl_trait)]
#![no_std]

#[cfg(feature = "multiboot2")]
pub mod multiboot2;
#[cfg(feature = "uefi")]
pub mod uefi;

use core::iter::Iterator;
use memory_structs::{PhysicalAddress, VirtualAddress};

pub trait MemoryRegion {
    /// Returns the region's starting physical address.
    fn start(&self) -> PhysicalAddress;

    /// Returns the region's length.
    fn len(&self) -> usize;

    /// Returns whether the region is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns whether the region can be used by the frame allocator.
    fn is_usable(&self) -> bool;
}

pub trait ElfSection {
    /// Returns the section's name.
    fn name(&self) -> &str;

    /// Returns the section's starting virtual address.
    fn start(&self) -> VirtualAddress;

    /// Returns the section's length in memory, as opposed to its length in the
    /// ELF file.
    fn len(&self) -> usize;

    /// Returns whether the section is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the section's flags.
    fn flags(&self) -> ElfSectionFlags;
}

bitflags::bitflags! {
    /// ELF section flags.
    pub struct ElfSectionFlags: u64 {
        /// The section contains data that should be writable during program execution.
        const WRITABLE = 0x1;

        /// The section occupies memory during the process execution.
        const ALLOCATED = 0x2;

        /// The section contains executable machine instructions.
        const EXECUTABLE = 0x4;
    }
}

pub trait Module {
    /// Returns the module's name.
    fn name(&self) -> Result<&str, &'static str>;

    /// Returns the module's starting physical address.
    fn start(&self) -> PhysicalAddress;

    /// Returns the module's length.
    fn len(&self) -> usize;

    /// Returns whether the module is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug)]
pub struct ReservedMemoryRegion {
    pub start: PhysicalAddress,
    pub len: usize,
}


/// Information about a framebuffer's layout in memory.
#[derive(Debug)]
pub struct FramebufferInfo {
    /// The virtual address of the start of the framebuffer,
    /// if it has been mapped for us by the bootloader.
    pub virt_addr: Option<VirtualAddress>,
    /// The physical address of the start of the framebuffer.
    pub phys_addr: PhysicalAddress,
    /// The total size of the framebuffer memory in bytes.
    pub total_size_in_bytes: u64,
    /// The width in pixels (number of columns) of the framebuffer.
    /// If this is a text framebuffer, this is in units of characters.
    pub width: u32,
    /// The height in pixels (number of rows) of the framebuffer.
    /// If this is a text framebuffer, this is in units of characters.
    pub height: u32,
    /// The number of bits that each pixel occupies in memory.
    /// If this is a text framebuffer, this is the number of bits that
    /// each character occupies in memory.
    pub bits_per_pixel: u8,
    /// The number of pixels between the start of one line (row)
    /// and the start of the next line (row).
    ///
    /// This is sometimes referred to as pixels per scan line.
    ///
    /// This is required because some framebuffer implementations
    /// may have padding (empty space) at the end of each line, i.e.,
    /// each line is not contiguous in memory.
    /// For such framebuffers, you must skip those padding pixels
    /// in order to get to the start of the next line in memory.
    ///
    /// * If `stride` is equal to `width`, there are no padding pixels.
    /// * If `stride` is greater than `width`, the number of padding pixels
    ///   after the end of each line is `stride - width`.
    /// * The value of `stride` itself is *NOT* the number of padding pixels.
    ///
    /// If this is a text framebuffer, this value represents the number of
    /// characters instead of pixels, but it is typically always 0.
    pub stride: u32,
    /// The format of the framebuffer and its pixels or characters.
    pub format: FramebufferFormat,
}

impl FramebufferInfo {
    /// Returns `true` if the bootloader mapped the framebuffer and
    /// can provide its virtual address.
    ///
    /// Returns `false` if the bootloader did not map the framebuffer and
    /// can only provide its physical address.
    pub fn is_mapped(&self) -> bool {
        self.virt_addr.is_some()
    }
}

/// The format of the framebuffer, in graphical pixels or text-mode characters.
#[derive(Clone, Copy, Debug)]
pub enum FramebufferFormat {
    /// The format of a pixel is `[Pad] <Red> <Green> <Blue>`,
    /// in which `<Blue>` occupies the least significant bits.
    ///
    /// Each pixel is 8 bits (1 byte), so the size of the padding bits
    /// is `bits_per_pixel - 24`.
    RgbPixel,
    /// The format of a pixel is `[Pad] <Blue> <Green> <Red>`,
    /// in which `<Red>` occupies the least significant bits.
    ///
    /// Each pixel is 8 bits (1 byte), so the size of the padding bits
    /// is `bits_per_pixel - 24`.
    BgrPixel,
    /// The format of a pixel is `[Pad] <Gray>`,
    /// in which `Gray` is a single byte representing a grayscale value.
    ///
    /// The size of the padding bits is `bits_per_pixel - 8`.
    Grayscale,
    /// The framebuffer is an [EGA] text-mode display comprised of 16-bit characters,
    /// not pixels.
    ///
    /// [EGA]: https://en.wikipedia.org/wiki/Enhanced_Graphics_Adapter
    TextCharacter,
    /// Custom pixel format of up to 32-bit pixels.
    CustomPixel {
        /// The bit position of the least significant bit of a pixel's red component.
        red_bit_position: u8,
        /// The size of a pixel's red component, in number of bits.
        red_size_in_bits: u8,
        /// The bit position of the least significant bit of a pixel's green component.
        green_bit_position: u8,
        /// The size of a pixel's green component, in number of bits.
        green_size_in_bits: u8,
        /// The bit position of the least significant bit of a pixel's blue component.
        blue_bit_position: u8,
        /// The size of a pixel's blue component, in number of bits.
        blue_size_in_bits: u8,
    },
}

pub trait BootInformation: 'static {
    type MemoryRegion<'a>: MemoryRegion;
    type MemoryRegions<'a>: Iterator<Item = Self::MemoryRegion<'a>>;

    type ElfSection<'a>: ElfSection;
    type ElfSections<'a>: Iterator<Item = Self::ElfSection<'a>>;

    type Module<'a>: Module;
    type Modules<'a>: Iterator<Item = Self::Module<'a>>;

    type AdditionalReservedMemoryRegions: Iterator<Item = ReservedMemoryRegion>;

    /// Returns the boot information's starting virtual address.
    fn start(&self) -> Option<VirtualAddress>;
    /// Returns the boot information's length.
    fn len(&self) -> usize;

    /// Returns whether the boot information is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns memory regions describing the physical memory.
    fn memory_regions(&self) -> Result<Self::MemoryRegions<'_>, &'static str>;
    /// Returns the kernel's ELF sections.
    fn elf_sections(&self) -> Result<Self::ElfSections<'_>, &'static str>;
    /// Returns the modules found in the kernel image.
    fn modules(&self) -> Self::Modules<'_>;

    /// Returns additional reserved memory regions that aren't included in
    /// the list of regions returned by [`memory_regions`].
    fn additional_reserved_memory_regions(
        &self,
    ) -> Result<Self::AdditionalReservedMemoryRegions, &'static str>;

    /// Returns the end of the kernel's image in memory.
    fn kernel_end(&self) -> Result<VirtualAddress, &'static str>;

    /// Returns the RSDP if it was provided by the bootloader.
    fn rsdp(&self) -> Option<PhysicalAddress>;

    /// Returns the stack size in bytes.
    fn stack_size(&self) -> Result<usize, &'static str>;

    /// Returns information about the graphical framebuffer, if available.
    fn framebuffer_info(&self) -> Option<FramebufferInfo>;
}
