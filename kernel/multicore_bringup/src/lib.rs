//! Functions for initializing and bringing up other CPU cores. 
//! 
//! These functions are intended to be invoked from the main core
//! (the BSP -- bootstrap core) in order to jumpstart other cores.

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate spin;
extern crate volatile;
extern crate zerocopy;
extern crate irq_safety;
extern crate memory;
extern crate pit_clock_basic;
extern crate stack;
extern crate kernel_config;
extern crate apic;
extern crate acpi;
extern crate madt;
extern crate mod_mgmt;
extern crate ap_start;
extern crate pause;

use core::{
    ops::DerefMut,
    sync::atomic::Ordering,
};
use alloc::sync::Arc;
use spin::Mutex;
use volatile::Volatile;
use zerocopy::FromBytes;
use irq_safety::MutexIrqSafe;
use memory::{VirtualAddress, PhysicalAddress, MappedPages, EntryFlags, MemoryManagementInfo};
use kernel_config::memory::{PAGE_SIZE, PAGE_SHIFT, KERNEL_STACK_SIZE_IN_PAGES};
use apic::{LocalApic, get_lapics, get_my_apic_id, has_x2apic, get_bsp_id};
use ap_start::{kstart_ap, AP_READY_FLAG};
use madt::{Madt, MadtEntry, MadtLocalApic, find_nmi_entry_for_processor};
use pause::spin_loop_hint;


/// The physical address that an AP jumps to when it first is booted by the BSP.
/// For x2apic systems, this must be at 0x10000 or higher! 
const AP_STARTUP: usize = 0x10000; 

/// The physical address of the memory area for AP startup data passed from the BSP in long mode (Rust) code.
/// Located one page below the AP_STARTUP code entry point, at 0xF000.
const TRAMPOLINE: usize = AP_STARTUP - PAGE_SIZE;

/// The offset from the `TRAMPOLINE` address to where the AP startup code will write `GraphicInfo`.
const GRAPHIC_INFO_OFFSET_FROM_TRAMPOLINE: usize = 0x100;

/// Graphic mode information that will be updated after `handle_ap_cores()` is invoked. 
pub static GRAPHIC_INFO: Mutex<GraphicInfo> = Mutex::new(GraphicInfo {
    width: 0,
    height: 0,
    physical_address: 0,
});

/// A structure to access information about the graphical framebuffer mode
/// that was discovered and chosen in the AP's real-mode initialization sequence.
/// 
/// # Struct format
/// The layout of fields in this struct must be kept in sync with the code in 
/// `ap_realmode.asm` that writes to this structure.
#[derive(FromBytes, Clone, Debug)]
pub struct GraphicInfo {
    pub width: u64,
    pub height: u64,
    pub physical_address: u64,
}

/// Starts up and sets up AP cores based on system information from ACPI
/// (specifically the MADT (APIC) table).
/// 
/// # Arguments: 
/// * `kernel_mmi_ref`: A reference to the locked MMI structure for the kernel.
/// * `ap_start_realmode_begin`: the starting virtual address of where the ap_start realmode code is.
/// * `ap_start_realmode_end`: the ending virtual address of where the ap_start realmode code is.
/// * `max_framebuffer_resolution`: the maximum resolution `(width, height)` of the graphical framebuffer
///    that an AP should request from the BIOS when it boots up in 16-bit real mode.
///    If `None`, there will be no maximum.
pub fn handle_ap_cores(
    kernel_mmi_ref: Arc<MutexIrqSafe<MemoryManagementInfo>>,
    ap_start_realmode_begin: VirtualAddress,
    ap_start_realmode_end: VirtualAddress,
    max_framebuffer_resolution: Option<(u16, u16)>,
) -> Result<usize, &'static str> {
    let ap_startup_size_in_bytes = ap_start_realmode_end.value() - ap_start_realmode_begin.value();

    let page_table_phys_addr: PhysicalAddress;
    let mut trampoline_mapped_pages: MappedPages; // must be held throughout APs being booted up
    let mut ap_startup_mapped_pages: MappedPages; // must be held throughout APs being booted up
    {
        let mut kernel_mmi = kernel_mmi_ref.lock();
        let page_table = &mut kernel_mmi.page_table;
        // first, double check that the ap_start_realmode address is mapped and valid
        page_table.translate(ap_start_realmode_begin).ok_or("handle_ap_cores(): couldn't translate ap_start_realmode address")?;

        // Map trampoline frame and the ap_startup code to the AP_STARTUP frame.
        // These frames MUST be identity mapped because they're accessed in AP boot up code,
        // which has no page tables because it operates in 16-bit real mode.
        let trampoline_frame  = memory::allocate_frames_at(PhysicalAddress::new_canonical(TRAMPOLINE), 1)
            .map_err(|_e| "handle_ap_cores(): failed to allocate trampoline frame")?;
        let trampoline_page   = memory::allocate_pages_at(VirtualAddress::new_canonical(TRAMPOLINE), trampoline_frame.size_in_frames())
            .map_err(|_e| "handle_ap_cores(): failed to allocate trampoline page")?;
        let ap_startup_frames = memory::allocate_frames_by_bytes_at(PhysicalAddress::new_canonical(AP_STARTUP), ap_startup_size_in_bytes)
            .map_err(|_e| "handle_ap_cores(): failed to allocate AP startup frames")?;
        let ap_startup_pages  = memory::allocate_pages_at(VirtualAddress::new_canonical(AP_STARTUP), ap_startup_frames.size_in_frames())
            .map_err(|_e| "handle_ap_cores(): failed to allocate AP startup pages")?;
        
        trampoline_mapped_pages = page_table.map_allocated_pages_to(
            trampoline_page, 
            trampoline_frame, 
            EntryFlags::PRESENT | EntryFlags::WRITABLE, 
        )?;
        ap_startup_mapped_pages = page_table.map_allocated_pages_to(
            ap_startup_pages,
            ap_startup_frames,
            EntryFlags::PRESENT | EntryFlags::WRITABLE,
        )?;
        page_table_phys_addr = page_table.physical_address();
    }

    let all_lapics = get_lapics();
    let me = get_my_apic_id();

    // Copy the AP startup code (from the kernel's text section pages) into the AP_STARTUP physical address entry point.
    {
        // First, get the kernel's text pages, which is the MappedPages object that contains the vaddr `ap_start_realmode_begin`.
        let kernel_text_pages_ref = mod_mgmt::get_initial_kernel_namespace()
            .ok_or("BUG: couldn't get the initial kernel CrateNamespace")
            .and_then(|namespace| namespace.get_crate("nano_core").ok_or("BUG: couldn't get the 'nano_core' crate"))
            .and_then(|nano_core_crate| nano_core_crate.lock_as_ref().text_pages.clone().ok_or("BUG: nano_core crate had no text pages"))?;
        let kernel_text_pages = kernel_text_pages_ref.0.lock();
        // Second, perform the actual copy.
        let source_slice: &[u8] = kernel_text_pages.offset_of_address(ap_start_realmode_begin)
            .ok_or("BUG: the 'ap_start_realmode_begin' virtual address was not covered by the kernel's text pages")
            .and_then(|offset| kernel_text_pages.as_slice(offset, ap_startup_size_in_bytes))?;
        let dest_slice: &mut [u8] = ap_startup_mapped_pages.as_slice_mut(0, ap_startup_size_in_bytes)?;
        dest_slice.copy_from_slice(source_slice);
    }
    // Now, the AP startup code is at the PhysicalAddress `AP_STARTUP`.

    let mut ap_count = 0; // the number of AP cores we have successfully booted.
    let ap_trampoline_data: &mut ApTrampolineData = trampoline_mapped_pages.as_type_mut(0)?;
    // Here, we set up the data items that will be accessible to the APs when they boot up.
    // We only set the values of fields that are the same for ALL APs here;
    // values that change for each AP are set individually in `bring_up_ap()` below.
    let (max_width, max_height) = max_framebuffer_resolution.unwrap_or((u16::MAX, u16::MAX));
    ap_trampoline_data.ap_max_fb_width.write(max_width);
    ap_trampoline_data.ap_max_fb_height.write(max_height);

    let acpi_tables = acpi::get_acpi_tables().lock();
    let madt = Madt::get(&acpi_tables)
        .ok_or("Couldn't find the MADT APIC table. Has the ACPI subsystem been initialized yet?")?;
    let madt_iter = madt.iter();

    for madt_entry in madt_iter.clone() {
        if let MadtEntry::LocalApic(lapic_entry) = madt_entry { 
            if lapic_entry.apic_id == me {
                // debug!("skipping BSP's local apic");
            }
            else {
                if lapic_entry.flags & 0x1 != 0x1 {
                    warn!("Processor {} apic_id {} is disabled by the hardware, cannot initialize or use it.", 
                            lapic_entry.processor, lapic_entry.apic_id);
                    continue;
                }

                // start up this AP, and have it create a new LocalApic for itself. 
                // This must be done by each core itself, and not called repeatedly by the BSP on behalf of other cores.
                let bsp_lapic_ref = get_bsp_id()
                    .and_then(|bsp_id| all_lapics.get(&bsp_id))
                    .ok_or("Couldn't get BSP's LocalApic!")?;
                let mut bsp_lapic = bsp_lapic_ref.write();
                let ap_stack = stack::alloc_stack(
                    KERNEL_STACK_SIZE_IN_PAGES,
                    &mut kernel_mmi_ref.lock().page_table,
                ).ok_or("could not allocate AP stack!")?;

                let (nmi_lint, nmi_flags) = find_nmi_entry_for_processor(lapic_entry.processor, madt_iter.clone());

                bring_up_ap(
                    bsp_lapic.deref_mut(), 
                    lapic_entry,
                    ap_trampoline_data,
                    page_table_phys_addr, 
                    ap_stack, 
                    nmi_lint,
                    nmi_flags 
                );
                ap_count += 1;
            }
        }
    }

    // Retrieve the graphic mode information written during the AP bootup sequence in `ap_realmode.asm`.
    {
        let graphic_info = trampoline_mapped_pages.as_type::<GraphicInfo>(GRAPHIC_INFO_OFFSET_FROM_TRAMPOLINE)?;
        info!("Obtained graphic info from real mode: {:?}", graphic_info);
        *GRAPHIC_INFO.lock() = graphic_info.clone();
    }
    
    // wait for all cores to finish booting and init
    info!("handle_ap_cores(): BSP is waiting for APs to boot...");
    let expected_cores = ap_count + 1;
    let mut num_known_cores = get_lapics().iter().count();
    let mut iter = 0;
    while num_known_cores < expected_cores {
        spin_loop_hint();
        num_known_cores = get_lapics().iter().count();
        if iter == 100000 {
            trace!("BSP is waiting for APs to boot ({} of {})", num_known_cores, expected_cores);
            iter = 0;
        }
        iter += 1;
    }
    
    Ok(ap_count)  
}


/// The data items used when an AP core is booting up in real mode.
///
/// # Important Layout Note
/// The order of the members in this struct must exactly match how they are used
/// and specified in the AP bootup code (at the top of `defines.asm`).
#[derive(FromBytes)]
#[repr(C)]
struct ApTrampolineData {
    /// A flag that indicates whether the new AP is ready. 
    /// The Rust setup code sets it to 0, and the AP boot code sets it to 1.
    ap_ready:          Volatile<u64>,
    /// The processor ID of the new AP that is being brought up.
    ap_processor_id:   Volatile<u8>,
    _padding0:         [u8; 7],
    /// The APIC ID of the new AP that is being brought up.
    ap_apic_id:        Volatile<u8>,
    _padding1:         [u8; 7],
    /// The physical address of the top-level P4 page table root (value of CR3).
    ap_page_table:     Volatile<PhysicalAddress>,
    /// The starting virtual address (bottom) of the stack that was allocated for the new AP.
    ap_stack_start:    Volatile<VirtualAddress>,
    /// The ending virtual address (top) of the stack that was allocated for the new AP.
    ap_stack_end:      Volatile<VirtualAddress>,
    /// The virtual address of the Rust entry point that the new AP should jump to after booting up.
    ap_code:           Volatile<VirtualAddress>,
    /// The NMI LINT (Non-Maskable Interrupt Local Interrupt) value for the new AP.
    ap_nmi_lint:       Volatile<u8>,
    _padding2:         [u8; 7],
    /// The NMI (Non-Maskable Interrupt) flags value for the new AP.
    ap_nmi_flags:      Volatile<u16>,
    _padding3:         [u8; 6],
    /// The maximum width in pixels of the graphical framebuffer that an AP should request
    /// when changing graphical framebuffer modes in its 16-bit real-mode code. 
    ap_max_fb_width:   Volatile<u16>,
    _padding4:         [u8; 6],
    /// The maximum height in pixels of the graphical framebuffer that an AP should request
    /// when changing graphical framebuffer modes in its 16-bit real-mode code. 
    ap_max_fb_height:  Volatile<u16>,
    _padding5:         [u8; 6],
}


/// Called by the BSP to initialize the given `new_lapic` using IPIs.
fn bring_up_ap(
    bsp_lapic: &mut LocalApic,
    new_lapic: &MadtLocalApic, 
    ap_trampoline_data: &mut ApTrampolineData,
    page_table_paddr: PhysicalAddress, 
    ap_stack: stack::Stack,
    nmi_lint: u8, 
    nmi_flags: u16
) {
    ap_trampoline_data.ap_ready.write(0);
    ap_trampoline_data.ap_processor_id.write(new_lapic.processor);
    ap_trampoline_data.ap_apic_id.write(new_lapic.apic_id);
    ap_trampoline_data.ap_page_table.write(page_table_paddr);
    ap_trampoline_data.ap_stack_start.write(ap_stack.bottom());
    ap_trampoline_data.ap_stack_end.write(ap_stack.top_unusable());
    ap_trampoline_data.ap_code.write(VirtualAddress::new_canonical(kstart_ap as usize));
    ap_trampoline_data.ap_nmi_lint.write(nmi_lint);
    ap_trampoline_data.ap_nmi_flags.write(nmi_flags);
    AP_READY_FLAG.store(false, Ordering::SeqCst);

    // Give ownership of the stack we created for this AP to the `ap_start` crate, 
    // in which the AP will take ownership of it once it boots up.
    ap_start::insert_ap_stack(new_lapic.apic_id, ap_stack); 

    info!("Bringing up AP, proc: {} apic_id: {}", new_lapic.processor, new_lapic.apic_id);
    let new_apic_id = new_lapic.apic_id; 
    
    bsp_lapic.clear_error();
    let esr = bsp_lapic.error();
    debug!(" initial esr = {:#X}", esr);

    // Send INIT IPI
    {
        // 0x500 means INIT Delivery Mode, 0x4000 means Assert (not de-assert), 0x8000 means level triggers
        let mut icr = /*0x8000 |*/ 0x4000 | 0x500; 
        if has_x2apic() {
            icr |= (new_apic_id as u64) << 32;
        } else {
            icr |= ( new_apic_id as u64) << 56; // destination apic id 
        }
        // icr |= 1 << 11; // (1 << 11) is logical address mode, 0 is physical. Doesn't work with physical addressing mode!
        debug!(" INIT IPI... icr: {:#X}", icr);
        bsp_lapic.set_icr(icr);
    }

    debug!("waiting 10 ms...");
    pit_clock_basic::pit_wait(10000).unwrap_or_else(|_e| { error!("bring_up_ap(): failed to pit_wait 10 ms. Error: {:?}", _e); });
    debug!("done waiting.");

    // // Send DEASSERT INIT IPI
    // {
    //     // 0x500 means INIT Delivery Mode, 0x8000 means level triggers
    //     let mut icr = 0x8000 | 0x500; 
    //     if has_x2apic() {
    //         icr |= (new_apic_id as u64) << 32;
    //     } else {
    //         icr |= ( new_apic_id as u64) << 56; // destination apic id 
    //     }
    //     // icr |= 1 << 11; // (1 << 11) is logical address mode, 0 is physical. Doesn't work with physical addressing mode!
    //     debug!(" DEASSERT IPI... icr: {:#X}", icr);
    //     bsp_lapic.set_icr(icr);
    // }

    bsp_lapic.clear_error();
    let esr = bsp_lapic.error();
    debug!(" pre-SIPI esr = {:#X}", esr);

    // Send START IPI
    {
        //Start at 0x1000:0000 => 0x10000. We copied the ap_start_realmode code into AP_STARTUP earlier, in handle_apic_entry()
        let ap_segment = (AP_STARTUP >> PAGE_SHIFT) & 0xFF; // the frame number where we want the AP to start executing from boot
        let mut icr = /*0x8000 |*/ 0x4000 | 0x600 | ap_segment as u64; //0x600 means Startup IPI

        if has_x2apic() {
            icr |= (new_apic_id as u64) << 32;
        } else {
            icr |= (new_apic_id as u64) << 56;
        }
        // icr |= 1 << 11; // (1 << 11) is logical address mode, 0 is physical. Doesn't work with physical addressing mode!
        debug!(" SIPI... icr: {:#X}", icr);
        bsp_lapic.set_icr(icr);
    }

    pit_clock_basic::pit_wait(300).unwrap_or_else(|_e| { error!("bring_up_ap(): failed to pit_wait 300 us. Error {:?}", _e); });
    pit_clock_basic::pit_wait(200).unwrap_or_else(|_e| { error!("bring_up_ap(): failed to pit_wait 200 us. Error {:?}", _e); });

    bsp_lapic.clear_error();
    let esr = bsp_lapic.error();
    debug!(" post-SIPI esr = {:#X}", esr);
    // TODO: we may need to send a second START IPI on real hardware???

    // Wait for trampoline ready
    debug!(" Wait...");
    while ap_trampoline_data.ap_ready.read() == 0 {
        spin_loop_hint();
    }
    debug!(" Trampoline...");
    while ! AP_READY_FLAG.load(Ordering::SeqCst) {
        spin_loop_hint();
    }
    info!(" AP {} is in Rust code. Ready!", new_apic_id);
}
