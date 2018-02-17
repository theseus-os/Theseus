use memory::{get_kernel_mmi_ref, FRAME_ALLOCATOR, MemoryManagementInfo, PhysicalAddress, VirtualAddress, Page, Frame, PageTable, EntryFlags,  

use core::ptr::{read_volatile, write_volatile};

use core::ops::DerefMut;





// get a reference to the kernel's memory mapping information

let kernel_mmi_ref = get_kernel_mmi_ref().expect("KERNEL_MMI was not yet initialized!");

let mut kernel_mmi_locked = kernel_mmi_ref.lock();



// destructure the kernel's MMI so we can access its page table

let MemoryManagementInfo { 

    page_table: ref mut kernel_page_table, 

    ..  // don't need to access other stuff in kernel_mmi

} = *kernel_mmi_locked;





let phys_addr = TODO put your phys addr here;

let virt_addr = TODO pick a VirtualAddress here; // it can't conflict with anything else, ask me for more info

let page = Page::containing_address(virt_addr);

let frame = Frame::containing_address(phys_addr as PhysicalAddress);

let mapping_flags = EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE;





// we can only map stuff if the kernel_page_table is Active

// which you can be guaranteed it will be if you're in kernel code

match kernel_page_table {

    &mut PageTable::Active(ref mut active_table) => {

        let mut frame_allocator = FRAME_ALLOCATOR.try().unwrap().lock();

        // this maps just one page to one frame (4KB). If you need to map more, ask me

        active_table.map_to(page, frame, mapping_flags, frame_allocator.deref_mut());



    }

    _ => { panic!("kernel page table wasn't an ActivePageTable!"); }

}





// Now that the registers (in physical memory) are mapped into virtual  memory, we can directly access them as usual

// In C-style code, it would just be something like  int val = *(virt_addr + register_offset);

// In Rust, we do it better



// If you want to read a 32-bit register, for example:

let val: u32 = unsafe { read_volatile((virt_addr + register_offset as usize) as *const u32) }; // "as *const u32" tells Rust what data type this pointer is pointing to



// Maybe you want to write a 64-bit value...

let new_value: u64 = 0x12353456;

unsafe { write_volatile((virt_addr + register_offset as usize) as *mut u64, new_value) }; // "as *const u32" tells Rust what data type this pointer is pointing to





// In apic.rs (in my apic branch only that I linked you), you can see that I've wrapped this up in an OOP-style design 

// where I create an object with the base MMIO register address, then create the memory mapping in an "init" or "new" function,

// and then just use "self.write_reg/read_reg" to make things easier to code. 

// You don't have to take that approach at first, you can code it in a simpler way just to get it to work, then refactor later. 
