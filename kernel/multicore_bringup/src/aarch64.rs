use memory::{MmiRef, VirtualAddress, PhysicalAddress, MappedPages, PteFlags};
use memory_aarch64::{read_mmu_config, asm_set_mmu_config_x2_x3};
use kernel_config::memory::{PAGE_SIZE, KERNEL_STACK_SIZE_IN_PAGES};
use psci::{cpu_on, error::Error::*};
use zerocopy::FromBytes;
use ap_start::kstart_ap;
use volatile::Volatile;
use core::arch::asm;
use cpu::{CpuId, MpidrValue, current_cpu};
use arm_boards::BOARD_CONFIG;

/// The data items used when an AP core is booting up in ap_entry_point & ap_stage_two.
#[cfg(target_arch = "aarch64")]
#[derive(Debug, Default, FromBytes)]
#[repr(C)]
struct ApTrampolineData {
    /// The ending virtual address (top) of the stack that was allocated for the new AP.
    /// Will initially be zero before the BSP sets it to a real value
    ap_stack_end: Volatile<VirtualAddress>,

    /// The starting virtual address (bottom) of the stack that was allocated for the new AP.
    ap_stack_start: Volatile<VirtualAddress>,

    // Address to jump to after the MMU has been configured
    ap_stage_two_virt_addr: Volatile<VirtualAddress>,

    // Address used to access this structure once in ap_stage_two
    ap_data_virt_addr: Volatile<VirtualAddress>,

    // Boolean set to true by ap_stage_two when it's done using this structure
    ap_ready: Volatile<u64>,

    /// The physical address of the top-level P4 page table root.
    ap_mmu_config: Volatile<PhysicalAddress>,
}

pub struct MulticoreBringupInfo;

pub fn handle_ap_cores(
    kernel_mmi_ref: &MmiRef,
    _multicore_info: MulticoreBringupInfo,
) -> Result<usize, &'static str> {
    let mut online_cores = 0;

    // This ApTrampolineData & MmuConfig will be read and written-to
    // by all detected CPU cores, via both its physical
    // and virtual addresses.
    let mmu_config = read_mmu_config();
    let mut ap_data: ApTrampolineData = Default::default();
    ap_data.ap_data_virt_addr.write(VirtualAddress::new_canonical(&ap_data as *const _ as usize));
    ap_data.ap_stage_two_virt_addr.write(VirtualAddress::new_canonical(ap_stage_two as usize));

    let entry_point_phys_addr: PhysicalAddress;
    let ap_data_phys_addr: PhysicalAddress;
    let mut ap_startup_mapped_pages: MappedPages; // must be held throughout APs being booted up
    {
        let mut kernel_mmi = kernel_mmi_ref.lock();
        let page_table = &mut kernel_mmi.page_table;

        // get the physical address of ap_entry_point
        let entry_point_virt_addr = VirtualAddress::new_canonical(ap_entry_point as usize);
        entry_point_phys_addr = page_table.translate(entry_point_virt_addr).unwrap();

        // get the physical address of the MmuConfig
        let mmu_config_virt_addr = VirtualAddress::new_canonical(&mmu_config as *const _ as usize);
        ap_data.ap_mmu_config.write(page_table.translate(mmu_config_virt_addr).unwrap());

        // Note: this could be variable on aarch64
        // we'd need a way to find any identity mappable page+frame range
        const AP_STARTUP: usize = 0x10000;

        // When the AArch64 core will enter startup code, it will do so with MMU disabled,
        // which means these frames MUST be identity mapped.
        let ap_startup_frames = memory::allocate_frames_by_bytes_at(PhysicalAddress::new_canonical(AP_STARTUP), PAGE_SIZE)
            .map_err(|_e| "handle_ap_cores(): failed to allocate AP startup frames")?;
        let ap_startup_pages  = memory::allocate_pages_at(VirtualAddress::new_canonical(AP_STARTUP), ap_startup_frames.size_in_frames())
            .map_err(|_e| "handle_ap_cores(): failed to allocate AP startup pages")?;

        // map this RWX
        let flags = PteFlags::new().valid(true).writable(true);
        ap_startup_mapped_pages = page_table.map_allocated_pages_to(
            ap_startup_pages,
            ap_startup_frames,
            flags,
        )?;

        // get physical address of the ApTrampolineData structure
        ap_data_phys_addr = page_table.translate(ap_data.ap_data_virt_addr.read()).unwrap();

        // copy the entry point code
        let dst = ap_startup_mapped_pages.as_slice_mut(0, PAGE_SIZE).unwrap();
        let src = unsafe { (ap_entry_point as *const [u8; PAGE_SIZE]).as_ref() }.unwrap();
        dst.copy_from_slice(src);
    }

    let mut ap_stack = None;
    for def_mpidr in BOARD_CONFIG.cpu_ids {
        let cpu_id = CpuId::from(def_mpidr);
        let mpidr = MpidrValue::from(cpu_id);

        ap_data.ap_ready.write(0);
        let stack = if let Some(stack) = ap_stack.take() {
            stack
        } else {
            // Create a new stack
            let stack = stack::alloc_stack(
                KERNEL_STACK_SIZE_IN_PAGES,
                &mut kernel_mmi_ref.lock().page_table,
            ).ok_or("could not allocate AP stack!")?;

            ap_data.ap_stack_start.write(stack.bottom());
            ap_data.ap_stack_end.write(stack.top_unusable());

            stack
        };

        // Associate the stack to this CpuId
        ap_start::insert_ap_stack(cpu_id.value(), stack);

        if let Err(kind) = cpu_on(mpidr.value(), entry_point_phys_addr.value() as _, ap_data_phys_addr.value() as _) {
            let msg = match kind {
                InvalidParameters => Some("InvalidParameters"),
                AlreadyOn => Some("AlreadyOn"),
                NotPresent => Some("NotPresent"),
                Disabled => Some("Disabled"),
                NotSupported => Some("NotSupported"),
                Denied => Some("Denied"),
                OnPending => Some("OnPending"),
                InternalFailure => Some("InternalFailure"),
                InvalidAddress => Some("InvalidAddress"),
                _ => Some("Unknown"),
            };

            // Re-take the stack we allocated for this CPU
            // so we can reuse it the next CPU.
            ap_stack = ap_start::take_ap_stack(cpu_id.value()).map(|s| s.into_inner());

            if let Some(msg) = msg {
                log::error!("Tried to start CPU core {} but got PSCI error: {}", cpu_id, msg);
            }
        } else {
            // Wait for the core to take note of the stack boundaries
            while ap_data.ap_ready.read() != 1 {}

            // ap_stack is None because the stack will be used by
            // the booting core; a new one will be created for the
            // next core

            // remember this CpuId
            online_cores += 1;
        }
    }

    Ok(online_cores)
}

/// The entry point for all secondary CPU cores, where they
/// jump with MMU disabled, thanks to PSCI. This function
/// must not make use of the stack.
///
/// The third argument to `psci::cpu_on` will be in x0 when
/// this is jumped to; we use this to pass the physical address
/// of an `ApTrampolineData` structure, which contains all we
/// need to bring the CPU core up.
///
/// Note: handle_ap_cores expects this code to be equal to or shorter than one page.
#[naked]
unsafe extern "C" fn ap_entry_point(_ap_data_ptr_in_x0: *mut ApTrampolineData) -> () {
    asm!(
        // unpack stack pointer
        "ldr x1, [x0, 0]",
        "mov sp, x1",

        // read mmu_config phys addr
        "ldr x2, [x0, 5*8]",

        // read ap_stage_two virt addr
        "ldr x1, [x0, 2*8]",

        // read ap_data virt addr
        "ldr x0, [x0, 3*8]",

        // set mmu config
        asm_set_mmu_config_x2_x3!(),
        // can't use phys addr below

        // jump to the virtual address of `ap_stage_two`
        // with the virtual address of the ApTrampolineData
        // structure in x0, so that `ap_stage_two` receives
        // it as its first argument.
        "br x1",
        options(noreturn)
    );
}

/// The second stage of AP configuration: extracting
/// info from the `ApTrampolineData` structure, signaling
/// that we're done with it to the BSP, and calling `kstart_ap`.
///
/// The MMU is configured and enabled before we reach this,
/// so we must use valid virtual addresses here.
unsafe extern "C" fn ap_stage_two(ap_data_ptr_in_x0: *mut ApTrampolineData) -> ! {
    let data = &mut *ap_data_ptr_in_x0;
    let ap_stack_start = data.ap_stack_start.read();
    let ap_stack_end   = data.ap_stack_end.read();
    data.ap_ready.write(1);
    kstart_ap(0, current_cpu(), ap_stack_start, ap_stack_end, 0, 0)
}
