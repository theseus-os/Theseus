#![no_std]

// #[macro_use] extern crate log;
#[macro_use] extern crate bitflags;
extern crate bit_field;
extern crate atomic_linked_list;
extern crate x86_64;
extern crate spin;
extern crate tss;
extern crate memory;

use core::ops::Deref;
use atomic_linked_list::atomic_map::AtomicMap;
use x86_64::{
    instructions::{
        segmentation::{CS, DS, SS, Segment},
        tables::load_tss,
    },
    PrivilegeLevel,
    structures::{
        tss::TaskStateSegment,
        gdt::SegmentSelector,
    },
    VirtAddr, 
};
use spin::Once;
use memory::VirtualAddress;


/// The GDT list, one per core, indexed by a key of apic_id
static GDT: AtomicMap<u8, Gdt> = AtomicMap::new();


static KERNEL_CODE_SELECTOR:  Once<SegmentSelector> = Once::new();
static KERNEL_DATA_SELECTOR:  Once<SegmentSelector> = Once::new();
static USER_CODE_32_SELECTOR: Once<SegmentSelector> = Once::new();
static USER_DATA_32_SELECTOR: Once<SegmentSelector> = Once::new();
static USER_CODE_64_SELECTOR: Once<SegmentSelector> = Once::new();
static USER_DATA_64_SELECTOR: Once<SegmentSelector> = Once::new();
static TSS_SELECTOR:          Once<SegmentSelector> = Once::new();


/// The GDT `SegmentSelector`s available in Theseus.
#[derive(Debug, Clone, Copy)]
pub enum AvailableSegmentSelector {
    KernelCode,
    KernelData,
    UserCode32,
    UserData32,
    UserCode64,
    UserData64,
    Tss,
}
impl AvailableSegmentSelector {
    /// Returns the requested `SegmentSelector`, or `None` if it hasn't yet been initialized.
    pub fn get(self) -> Option<SegmentSelector> {
        match self {
            AvailableSegmentSelector::KernelCode => KERNEL_CODE_SELECTOR.get().cloned(),
            AvailableSegmentSelector::KernelData => KERNEL_DATA_SELECTOR.get().cloned(),
            AvailableSegmentSelector::UserCode32 => USER_CODE_32_SELECTOR.get().cloned(),
            AvailableSegmentSelector::UserData32 => USER_DATA_32_SELECTOR.get().cloned(),
            AvailableSegmentSelector::UserCode64 => USER_CODE_64_SELECTOR.get().cloned(),
            AvailableSegmentSelector::UserData64 => USER_DATA_64_SELECTOR.get().cloned(),
            AvailableSegmentSelector::Tss        => TSS_SELECTOR.get().cloned(),
        }
    }
}


/// This function first creates and sets up a new TSS with the given double fault stack and privilege stack.
///
/// It then creates a new GDT with an entry that references that TSS and loads that new GDT into memory. 
///
/// Finally, it switches the various code and segment selectors to use that new GDT.
///
/// # Important Note
/// The GDT entries (segment descriptors) are only created **once** upon first invocation of this function,
/// such that the segment selectors are usable 
/// Future invocations will not change those initial values and load the same GDT based on them.
pub fn create_and_load_tss_gdt(
    apic_id: u8, 
    double_fault_stack_top_unusable: VirtualAddress, 
    privilege_stack_top_unusable: VirtualAddress
) { 
    let tss_ref = tss::create_tss(apic_id, double_fault_stack_top_unusable, privilege_stack_top_unusable);
    let (gdt, kernel_cs, kernel_ds, user_cs_32, user_ds_32, user_cs_64, user_ds_64, tss_segment) 
        = create_gdt(tss_ref.lock().deref());

    KERNEL_CODE_SELECTOR .call_once(|| kernel_cs);
    KERNEL_DATA_SELECTOR .call_once(|| kernel_ds);
    USER_CODE_32_SELECTOR.call_once(|| user_cs_32);
    USER_DATA_32_SELECTOR.call_once(|| user_ds_32);
    USER_CODE_64_SELECTOR.call_once(|| user_cs_64);
    USER_DATA_64_SELECTOR.call_once(|| user_ds_64);
    TSS_SELECTOR         .call_once(|| tss_segment);

    GDT.insert(apic_id, gdt);
    let gdt_ref = GDT.get(&apic_id).unwrap(); // safe to unwrap since we just added it to the list
    gdt_ref.load();
    // debug!("Loaded GDT for apic {}: {}", apic_id, gdt_ref);

    unsafe {
        CS::set_reg(kernel_cs);  // reload code segment register
        load_tss(tss_segment);   // load TSS
        SS::set_reg(kernel_ds);  // unsure if necessary, but doesn't hurt
        DS::set_reg(kernel_ds);  // unsure if necessary, but doesn't hurt
    }
}


/// Creates and sets up a new GDT that refers to the given `TSS`. 
///
/// Returns a tuple including:
/// 1. the new GDT
/// 2. kernel code segment selector
/// 3. kernel data segment selector
/// 4. user 32-bit code segment selector
/// 5. user 32-bit data segment selector
/// 6. user 64-bit code segment selector
/// 7. user 64-bit data segment selector
/// 8. tss segment selector
pub fn create_gdt(tss: &TaskStateSegment) -> (
    Gdt, SegmentSelector, SegmentSelector, SegmentSelector, 
    SegmentSelector, SegmentSelector, SegmentSelector, SegmentSelector
) {
    let mut gdt = Gdt::new();

    // The following order of segments must be preserved: 
    // 0)   null descriptor  (ensured by the Gdt type constructor)
    // 1)   kernel code segment
    // 2)   kernel data segment
    // 3)   user 32-bit code segment
    // 4)   user 32-bit data segment
    // 5)   user 64-bit code segment
    // 6)   user 64-bit data segment
    // 7-8) tss segment
    //
    // DO NOT rearrange the below calls to gdt.add_entry(), x86_64 has **VERY PARTICULAR** rules about this

    let kernel_cs   = gdt.add_entry(Descriptor::kernel_code_segment(),  PrivilegeLevel::Ring0);
    let kernel_ds   = gdt.add_entry(Descriptor::kernel_data_segment(),  PrivilegeLevel::Ring0);
    let user_cs_32  = gdt.add_entry(Descriptor::user_code_32_segment(), PrivilegeLevel::Ring3);
    let user_ds_32  = gdt.add_entry(Descriptor::user_data_32_segment(), PrivilegeLevel::Ring3);
    let user_cs_64  = gdt.add_entry(Descriptor::user_code_64_segment(), PrivilegeLevel::Ring3);
    let user_ds_64  = gdt.add_entry(Descriptor::user_data_64_segment(), PrivilegeLevel::Ring3);
    let tss_segment = gdt.add_entry(Descriptor::tss_segment(tss),       PrivilegeLevel::Ring0);

    (gdt, kernel_cs, kernel_ds, user_cs_32, user_ds_32, user_cs_64, user_ds_64, tss_segment)
}

/// The Global Descriptor Table, as specified by the x86_64 architecture.
/// 
/// See more info about GDT [here](http://wiki.osdev.org/Global_Descriptor_Table)
/// and [here](http://www.flingos.co.uk/docs/reference/Global-Descriptor-Table/).
pub struct Gdt {
    table: [u64; 10],  // max size is 8192 entries, but we don't need that many.
    next_free: usize,
}

impl Gdt {
    pub const fn new() -> Gdt {
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
            base: VirtAddr::new(self.table.as_ptr() as u64),
            limit: (self.table.len() * size_of::<u64>() - 1) as u16,
        };

        unsafe { lgdt(&ptr) };
    }
}

use core::fmt;
impl fmt::Display for Gdt {
    fn fmt(&self, fmtr: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        fmtr.write_fmt(format_args!("\nGdt: [\n"))?;
        for (index, entry) in self.table.iter().enumerate() {
            #[allow(clippy::uninlined_format_args)]
            fmtr.write_fmt(format_args!("  {}:  {:#016x}\n", index, entry))?;
        }
        fmtr.write_fmt(format_args!("]"))?;
        Ok(())
    }
}

/// The two kinds of descriptor entries in the GDT.
pub enum Descriptor {
    /// UserSegment is used for both code and data segments, 
    /// in both the kernel and in user space.
    UserSegment(u64),
    /// SystemSegment is used only for TSS.
    SystemSegment(u64, u64),
}

impl Descriptor {
    pub const fn kernel_code_segment() -> Descriptor {
        let flags = DescriptorFlags::LONG_MODE.bits() | 
                    DescriptorFlags::PRESENT.bits() | 
                    DescriptorFlags::PRIVILEGE_RING0.bits() | 
                    DescriptorFlags::USER_SEGMENT.bits() | 
                    DescriptorFlags::EXECUTABLE.bits() | 
                    DescriptorFlags::READ_WRITE.bits();
        Descriptor::UserSegment(flags)
    }

    pub const fn kernel_data_segment() -> Descriptor {
        let flags = DescriptorFlags::PRESENT.bits() | 
                    DescriptorFlags::PRIVILEGE_RING0.bits() | 
                    DescriptorFlags::USER_SEGMENT.bits() | 
                    DescriptorFlags::READ_WRITE.bits(); 
        Descriptor::UserSegment(flags)
    }

    pub const fn user_code_32_segment() -> Descriptor {
        let flags = DescriptorFlags::SIZE.bits() | 
                    DescriptorFlags::PRESENT.bits() | 
                    DescriptorFlags::PRIVILEGE_RING3.bits() | 
                    DescriptorFlags::USER_SEGMENT.bits() | 
                    DescriptorFlags::EXECUTABLE.bits();
        Descriptor::UserSegment(flags)
    }

    pub const fn user_data_32_segment() -> Descriptor {
        let flags = DescriptorFlags::SIZE.bits() | 
                    DescriptorFlags::PRESENT.bits() | 
                    DescriptorFlags::PRIVILEGE_RING3.bits() | 
                    DescriptorFlags::USER_SEGMENT.bits() | 
                    DescriptorFlags::READ_WRITE.bits(); 
        Descriptor::UserSegment(flags)
    }

    pub const fn user_code_64_segment() -> Descriptor {
        let flags = DescriptorFlags::LONG_MODE.bits() | 
                    DescriptorFlags::PRESENT.bits() | 
                    DescriptorFlags::PRIVILEGE_RING3.bits() | 
                    DescriptorFlags::USER_SEGMENT.bits() | 
                    DescriptorFlags::EXECUTABLE.bits();
        Descriptor::UserSegment(flags)
    }

    pub const fn user_data_64_segment() -> Descriptor {
        let flags = DescriptorFlags::PRESENT.bits() | 
                    DescriptorFlags::PRIVILEGE_RING3.bits() | 
                    DescriptorFlags::USER_SEGMENT.bits() | 
                    DescriptorFlags::READ_WRITE.bits(); 
        Descriptor::UserSegment(flags)
    }
    

    pub fn tss_segment(tss: &TaskStateSegment) -> Descriptor {
        use core::mem::size_of;
        use bit_field::BitField;

        let ptr = tss as *const _ as u64;

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
