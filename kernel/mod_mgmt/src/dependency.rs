//! Structures for representing dependencies between sections.
//! 
//! Dependencies work as follows:
//!  
//! If one section `A` references or uses another section `B`, 
//! then we colloquially say that *`A` depends on `B`*. 
//! 
//! In this scenario, `A` has a `StrongDependency` on `B`,
//! and `B` has a `WeakDependent` pointing back to `A`. 
//! 
//! Assuming `A` and `B` are both `LoadedSection` objects,
//! then `B.sections_i_depend_on` includes a `StrongDependency(A)`
//! and `A.sections_dependent_on_me` includes a `WeakDependent(B)`.
//!  
//! In this way, the dependency graphs are fully associative,
//! allowing a given `LoadedSection` to easily find 
//! both its dependencies and its dependents instantly.
//! 
//! More importantly, it allows `A` to be dropped before `B`, 
//! but not the other way around. 
//! This correctly avoids dependency violations by ensuring that a section `B`
//! is never dropped while any other section `A` relies on it.
//! 
//! When swapping crates, the `WeakDependent`s are actually more useful. 
//! For example, if we want to swap the crate that contains section `B1` with a new one `B2`, 
//! then we can immediately find all of the section `A`s that depend on `B1` 
//! by iterating over `B1.sections_dependent_on_me`. 
//! To complete the swap and fully replace `B1` with `B2`, 
//! we would do the following (pseudocode):
//! ```
//! for secA in B1.sections_dependent_on_me {     
//!     change secA's relocation to point to B1     
//!     add WeakDependent(secA) to B2.sections_dependent_on_me     
//!     remove StrongDependency(B1) from secA.sections_i_depend_on     
//!     add StrongDependency(B2) to secA.sections_i_depend_on      
//!     remove WeakDependent(secA) from B1.sections_dependent_on_me (current iterator)     
//! }
//! ```
//! 

use xmas_elf;
use metadata::{StrongSectionRef, WeakSectionRef};
use goblin::elf::reloc::*;

/// A representation that the owner `A` of (a `LoadedSection` object containing) this struct
/// depends on the given `section` `B` in this struct.
/// The dependent section `A` is not specifically included here;
/// since it's the owner of this struct, it's implicit that it's the dependent one.
///  
/// A dependency is a strong reference to another `LoadedSection` `B`,
/// because that other section `B` shouldn't be removed as long as there are still sections (`A`) that depend on it.
/// 
/// This is the inverse of the [`WeakDependency`](#struct.WeakDependency) type.
#[derive(Debug, Clone)]
pub struct StrongDependency {
    /// A strong reference to the `LoadedSection` `B` that the owner of this struct (`A`) depends on.
    pub section: StrongSectionRef,
    /// The details of the relocation action that was performed.
    pub relocation: RelocationEntry,
}


/// A representation that the `section` `A` in this struct
/// depends on the owner `B` of (the `LoadedSection` object containing) this struct. 
/// The target dependency `B` is not specifically included here; 
/// it's implicitly the owner of this struct.
///  
/// This is a weak reference to another `LoadedSection` `A`,
/// because it is okay to remove a section `A` that depends on the owning section `B` before removing `B`.
/// Otherwise, there would be an infinitely recursive dependency, and neither `A` nor `B` could ever be dropped.
/// This design allows for `A` to be dropped before `B`, because there is no dependency ordering violation there.
/// 
/// This is the inverse of the [`StrongDependency`](#struct.StrongDependency) type.
#[derive(Debug, Clone)]
pub struct WeakDependent {
    /// A weak reference to the `LoadedSection` `A` that depends on the owner `B` of this struct.
    pub section: WeakSectionRef,
    /// The details of the relocation action that was performed.
    pub relocation: RelocationEntry,
}


/// The information necessary to calculate and write a relocation value,
/// based on a source section and a target section, in which a value 
/// based on the location of the source section is written somwhere in the target section.
#[derive(Debug, Copy, Clone)]
pub struct RelocationEntry {
    /// The type of relocation calculation that is performed 
    /// to connect the target section to the source section.
    pub typ: u32,
    /// The value that is added to the source section's address 
    /// when performing the calculation of the source value that is written to the target section.
    pub addend: usize,
    /// The offset from the starting virtual address of the target section
    /// that specifies where the relocation value should be written.
    pub offset: usize,
}
impl RelocationEntry {
    pub fn from_elf_relocation(rela_entry: &xmas_elf::sections::Rela<u64>) -> RelocationEntry {
        RelocationEntry {
            typ: rela_entry.get_type(),
            addend: rela_entry.get_addend() as usize,
            offset: rela_entry.get_offset() as usize,
        }
    }

    /// Returns true if the relocation type results in a relocation calculation
    /// in which the source value written into the target section 
    /// does NOT depend on the target section's address itself in any way 
    /// (i.e., it only depends on the source section)
    pub fn is_absolute(&self) -> bool {
        match self.typ {
            R_X86_64_32 | 
            R_X86_64_64 => true,
            _ => false,
        }
    }
}


/// A representation that the section that owns this struct 
/// has a dependency on the given `source_sec`, *in the same crate*.
/// The dependency itself is specified via the other section's shndx.
#[derive(Debug, Clone)]
pub struct InternalDependency {
    pub relocation: RelocationEntry,
    pub source_sec_shndx: usize,
}
impl InternalDependency {
    pub fn new(relocation: RelocationEntry, source_sec_shndx: usize) -> InternalDependency {
        InternalDependency {
            relocation, source_sec_shndx
        }
    }
}
