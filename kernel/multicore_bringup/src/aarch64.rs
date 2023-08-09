use memory::{create_identity_mapping, MmiRef, VirtualAddress, PhysicalAddress, PteFlags};
use memory_aarch64::{read_mmu_config, asm_set_mmu_config_x2_x3};
use kernel_config::memory::{PAGE_SIZE, KERNEL_STACK_SIZE_IN_PAGES};
use psci::{cpu_on, error::Error::*};
use zerocopy::FromBytes;
use ap_start::kstart_ap;
use volatile::Volatile;
use core::arch::asm;
use cpu::{CpuId, MpidrValue, current_cpu};
use arm_boards::BOARD_CONFIG;
use mod_mgmt::get_initial_kernel_namespace;

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
) -> Result<u32, &'static str> {
    let mut online_secondary_cpus = 0;

    // This ApTrampolineData & MmuConfig will be read and written to
    // by all detected CPU cores, via both its physical and virtual addresses.
    let mmu_config = read_mmu_config();
    let mut ap_data: ApTrampolineData = Default::default();
    ap_data.ap_data_virt_addr.write(VirtualAddress::new_canonical(&ap_data as *const _ as usize));
    ap_data.ap_stage_two_virt_addr.write(VirtualAddress::new_canonical(ap_stage_two as usize));

    // Identity map one page of memory and copy the executable code of `ap_entry_point` into it.
    // This ensures that when we run that `ap_entry_point` code from the identity-mapped page,
    // it can safely enable the MMU, as the program counter will be valid
    // (and have the same value) both before and after the MMU is enabled.
    let rwx = PteFlags::new().valid(true).writable(true).executable(true);
    let mut ap_startup_mapped_pages = create_identity_mapping(1, rwx)?;
    let virt_addr = ap_startup_mapped_pages.start_address();

    {
        let kernel_text_pages_ref = get_initial_kernel_namespace()
            .ok_or("BUG: couldn't get the initial kernel CrateNamespace")
            .and_then(|namespace| namespace.get_crate("nano_core").ok_or("BUG: couldn't get the 'nano_core' crate"))
            .and_then(|nano_core_crate| nano_core_crate.lock_as_ref().text_pages.clone().ok_or("BUG: nano_core crate had no text pages"))?;
        let kernel_text_pages = kernel_text_pages_ref.0.lock();

        let ap_entry_point = VirtualAddress::new_canonical(ap_entry_point as usize);
        let src = kernel_text_pages.offset_of_address(ap_entry_point)
            .ok_or("BUG: the 'ap_entry_point' virtual address was not covered by the kernel's text pages")
            .and_then(|offset| kernel_text_pages.as_slice(offset, PAGE_SIZE))?;

        let dst: &mut [u8] = ap_startup_mapped_pages.as_slice_mut(0, PAGE_SIZE)?;
        dst.copy_from_slice(src);

        // After copying the content into the identity page, remap it to remove write permissions.
        let mut kernel_mmi = kernel_mmi_ref.lock();
        let rx = PteFlags::new().valid(true).executable(true);
        ap_startup_mapped_pages.remap(&mut kernel_mmi.page_table, rx)?;
    }

    // We identity mapped the `ap_entry_point` above, but we need to translate
    // the virtual address of the `ap_data` in order to obtain its physical address.
    let entry_point_phys_addr = PhysicalAddress::new_canonical(virt_addr.value());
    let ap_data_phys_addr = {
        let mut kernel_mmi = kernel_mmi_ref.lock();
        let page_table = &mut kernel_mmi.page_table;

        // Write the physical address of the MmuConfig struct into the ApData struct.
        let mmu_config_virt_addr = VirtualAddress::new_canonical(&mmu_config as *const _ as usize);
        ap_data.ap_mmu_config.write(page_table.translate(mmu_config_virt_addr).unwrap());

        // get physical address of the ApTrampolineData structure
        page_table.translate(ap_data.ap_data_virt_addr.read()).unwrap()
    };

    let mut ap_stack = None;
    for def_mpidr in BOARD_CONFIG.cpu_ids {
        let cpu_id = CpuId::from(def_mpidr);
        let mpidr = MpidrValue::from(cpu_id);

        ap_data.ap_ready.write(0);
        let stack = if let Some(stack) = ap_stack.take() {
            stack
        } else {
            // Create a new stack for the CPU to use upon boot.
            let stack = stack::alloc_stack(
                KERNEL_STACK_SIZE_IN_PAGES,
                &mut kernel_mmi_ref.lock().page_table,
            ).ok_or("could not allocate AP stack!")?;

            ap_data.ap_stack_start.write(stack.bottom());
            ap_data.ap_stack_end.write(stack.top_unusable());

            stack
        };

        // Make the stack available for use by the target CPU.
        ap_start::insert_ap_stack(cpu_id.value(), stack);

        log::trace!("Calling cpu_on(MPIDR: {:#X}, entry: {:#X}, context: {:#X}",
            mpidr, entry_point_phys_addr, ap_data_phys_addr,
        );
        match cpu_on(
            mpidr.value(),
            entry_point_phys_addr.value() as u64,
            ap_data_phys_addr.value() as u64)
        {
            Ok(()) => {
                // Wait for the CPU to boot and enter Rust code.
                while ap_data.ap_ready.read() != 1 {
                    log::trace!("waiting for AP {} to boot...", mpidr);
                    core::hint::spin_loop();
                }

                // Here, `ap_stack` is None, indicating the `stack` is being used by
                // the CPU being booted. A new stack will be allocated for the next CPU.

                // Treat this CPU as booted and online.
                online_secondary_cpus += 1;
            }
            Err(psci_error) => {
                // Re-take the stack we allocated for this CPU
                // so we can reuse it the next CPU.
                ap_stack = ap_start::take_ap_stack(cpu_id.value()).map(|s| s.into_inner());

                match psci_error {
                    AlreadyOn => log::info!("CPU {} was already on.", cpu_id),
                    other => log::error!("Failed to boot CPU {}, PSCI error: {:?}", cpu_id, other),
                }
            }
        }
    }

    Ok(online_secondary_cpus)
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
unsafe extern "C" fn ap_entry_point(_ap_data_ptr_in_x0: *mut ApTrampolineData) {
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
