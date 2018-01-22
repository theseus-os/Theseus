use x86_64::structures::tss::TaskStateSegment;
use x86_64::structures::gdt::SegmentSelector;
use x86_64::PrivilegeLevel;


pub struct Gdt {
    table: [u64; 10],  // max size is 8192 entries, but we don't need that many.
    next_free: usize,
}

impl Gdt {
    pub fn new() -> Gdt {
        Gdt {
            table: [0; 10], 
            next_free: 1, // skip the 0th entry because that must be null
        }
    }

    pub fn add_entry(&mut self, entry: Descriptor, privilege: PrivilegeLevel) -> SegmentSelector {
        let index = match entry {
            Descriptor::UserSegment(value) => self.push(value),
            Descriptor::SystemSegment(value_low, value_high) => {
                let index = self.push(value_low);
                self.push(value_high);
                index
            }
        };
        SegmentSelector::new(index as u16, privilege)
    }

    fn push(&mut self, value: u64) -> usize {
        if self.next_free < self.table.len() {
            let index = self.next_free;
            self.table[index] = value;
            self.next_free += 1;
            index
        } else {
            panic!("GDT full");
        }
    }

    pub fn load(&self) {
        use x86_64::instructions::tables::{DescriptorTablePointer, lgdt};
        use core::mem::size_of;

        let ptr = DescriptorTablePointer {
            base: self.table.as_ptr() as u64,
            limit: (self.table.len() * size_of::<u64>() - 1) as u16,
        };

        unsafe { lgdt(&ptr) };
    }
}

use core::fmt;
impl fmt::Display for Gdt {
    fn fmt(&self, fmtr: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        fmtr.write_fmt(format_args!("\nGdt: [\n"));
        for (index, entry) in self.table.iter().enumerate() {
            try!( fmtr.write_fmt(format_args!("  {}:  {:#016x}\n", index, entry)));
        }
        fmtr.write_fmt(format_args!("]"));
        Ok(())
    }
}

/// We need 6 GDT segments even for 64-bit: http://tunes.org/~qz/search/?view=1&c=osdev&y=17&m=1&d=2
/// See more info about GDT here: http://www.flingos.co.uk/docs/reference/Global-Descriptor-Table/
///                     and here: http://wiki.osdev.org/Global_Descriptor_Table
pub enum Descriptor {
    /// UserSegment is used for both code and data segments, 
    /// in both the kernel and in user space
    UserSegment(u64),
    /// SystemSegment is used only for TSS
    SystemSegment(u64, u64),
}

impl Descriptor {
    pub fn kernel_code_segment() -> Descriptor {
        let flags = DescriptorFlags::LONG_MODE | 
                    DescriptorFlags::PRESENT | 
                    DescriptorFlags::PRIVILEGE_RING0 | 
                    DescriptorFlags::USER_SEGMENT | 
                    DescriptorFlags::EXECUTABLE | 
                    DescriptorFlags::READ_WRITE;
        Descriptor::UserSegment(flags.bits())
    }

    pub fn kernel_data_segment() -> Descriptor {
        let flags = DescriptorFlags::PRESENT | 
                    DescriptorFlags::PRIVILEGE_RING0 | 
                    DescriptorFlags::USER_SEGMENT | 
                    DescriptorFlags::READ_WRITE; 
                    // | DescriptorFlags::ACCESSED;
        Descriptor::UserSegment(flags.bits())
    }

    pub fn user_code_32_segment() -> Descriptor {
        let flags = DescriptorFlags::SIZE | 
                    DescriptorFlags::PRESENT | 
                    DescriptorFlags::PRIVILEGE_RING3 | 
                    DescriptorFlags::USER_SEGMENT | 
                    DescriptorFlags::EXECUTABLE;
        Descriptor::UserSegment(flags.bits())
    }

    pub fn user_data_32_segment() -> Descriptor {
        let flags = DescriptorFlags::SIZE | 
                    DescriptorFlags::PRESENT | 
                    DescriptorFlags::PRIVILEGE_RING3 | 
                    DescriptorFlags::USER_SEGMENT | 
                    DescriptorFlags::READ_WRITE; 
                    // | DescriptorFlags::ACCESSED;
        Descriptor::UserSegment(flags.bits())
    }

    pub fn user_code_64_segment() -> Descriptor {
        let flags = DescriptorFlags::LONG_MODE | 
                    DescriptorFlags::PRESENT | 
                    DescriptorFlags::PRIVILEGE_RING3 | 
                    DescriptorFlags::USER_SEGMENT | 
                    DescriptorFlags::EXECUTABLE;
        Descriptor::UserSegment(flags.bits())
    }

    pub fn user_data_64_segment() -> Descriptor {
        let flags = DescriptorFlags::PRESENT | 
                    DescriptorFlags::PRIVILEGE_RING3 | 
                    DescriptorFlags::USER_SEGMENT | 
                    DescriptorFlags::READ_WRITE; 
                    // | DescriptorFlags::ACCESSED;
        Descriptor::UserSegment(flags.bits())
    }
    

    // pub fn tss_segment(tss: &'static TaskStateSegment) -> Descriptor {
    pub fn tss_segment(ptr: u64) -> Descriptor {
        use core::mem::size_of;
        use bit_field::BitField;

        let mut low = DescriptorFlags::PRESENT.bits();
        // base
        low.set_bits(16..40, ptr.get_bits(0..24));
        low.set_bits(56..64, ptr.get_bits(24..32));
        // limit (the `-1` in needed since the bound is inclusive)
        low.set_bits(0..16, (size_of::<TaskStateSegment>() - 1) as u64);
        // type (0b1001 = available 64-bit tss)
        low.set_bits(40..44, 0b1001);

        let mut high = 0;
        high.set_bits(0..32, ptr.get_bits(32..64));

        Descriptor::SystemSegment(low, high)
    }
}

bitflags! {
    struct DescriptorFlags: u64 {
        const ACCESSED          = 1 << 40; // should always be zero, don't use this
        const READ_WRITE        = 1 << 41; // ignored by 64-bit CPU modes
        // const _CONFORMING       = 1 << 42; // not used yet ??
        const EXECUTABLE        = 1 << 43; // should be 1 for code segments, 0 for data segments
        const USER_SEGMENT      = 1 << 44; 
        const PRIVILEGE_RING0   = 0 << 45; // sets 45 and 46
        const PRIVILEGE_RING1   = 1 << 45; // sets 45 and 46
        const PRIVILEGE_RING2   = 2 << 45; // sets 45 and 46
        const PRIVILEGE_RING3   = 3 << 45; // sets 45 and 46
        // bit 46 is set above by PRIVILEGE_RING#
        const PRESENT           = 1 << 47;
        const LONG_MODE         = 1 << 53; // data segments should set this bit to 0
        const SIZE              = 1 << 54; // set to 1 for 32-bit segments, otherwise 0.
    }
}
