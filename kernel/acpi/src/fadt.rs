use core::{mem, ptr};

use super::sdt::Sdt;
use super::{ACPI_TABLE, SDT_POINTERS, get_sdt, find_matching_sdts, get_sdt_signature, load_table};

use memory::ActivePageTable;

#[repr(packed)]
#[derive(Debug)]
pub struct Fadt {
    pub header: Sdt,
    pub firmware_ctrl: u32,
    pub dsdt: u32,

    // field used in ACPI 1.0; no longer in use, for compatibility only
    reserved: u8,

    pub preferred_power_managament: u8,
    pub sci_interrupt: u16,
    pub smi_command_port: u32,
    pub acpi_enable: u8,
    pub acpi_disable: u8,
    pub s4_bios_req: u8,
    pub pstate_control: u8,
    pub pm1a_event_block: u32,
    pub pm1b_event_block: u32,
    pub pm1a_control_block: u32,
    pub pm1b_control_block: u32,
    pub pm2_control_block: u32,
    pub pm_timer_block: u32,
    pub gpe0_block: u32,
    pub gpe1_block: u32,
    pub pm1_event_length: u8,
    pub pm1_control_length: u8,
    pub pm2_control_length: u8,
    pub pm_timer_length: u8,
    pub gpe0_ength: u8,
    pub gpe1_length: u8,
    pub gpe1_base: u8,
    pub c_state_control: u8,
    pub worst_c2_latency: u16,
    pub worst_c3_latency: u16,
    pub flush_size: u16,
    pub flush_stride: u16,
    pub duty_offset: u8,
    pub duty_width: u8,
    pub day_alarm: u8,
    pub month_alarm: u8,
    pub century: u8,

    // reserved in ACPI 1.0; used since ACPI 2.0+
    pub boot_architecture_flags: u16,

    reserved2: u8,
    pub flags: u32,
}

/* ACPI 2 structure
#[repr(packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct GenericAddressStructure {
    address_space: u8,
    bit_width: u8,
    bit_offset: u8,
    access_size: u8,
    address: u64,
}

{
    // 12 byte structure; see below for details
    pub reset_reg: GenericAddressStructure,

    pub reset_value: u8,
    reserved3: [u8; 3],

    // 64bit pointers - Available on ACPI 2.0+
    pub x_firmware_control: u64,
    pub x_dsdt: u64,

    pub x_pm1a_event_block: GenericAddressStructure,
    pub x_pm1b_event_block: GenericAddressStructure,
    pub x_pm1a_control_block: GenericAddressStructure,
    pub x_pm1b_control_block: GenericAddressStructure,
    pub x_pm2_control_block: GenericAddressStructure,
    pub x_pm_timer_block: GenericAddressStructure,
    pub x_gpe0_block: GenericAddressStructure,
    pub x_gpe1_block: GenericAddressStructure,
}
*/

impl Fadt {
    pub fn new(sdt: &'static Sdt) -> Option<Fadt> {
        if &sdt.signature == b"FACP" && sdt.length as usize >= mem::size_of::<Fadt>() {
            Some(unsafe { ptr::read((sdt as *const Sdt) as *const Fadt) })
        } else {
            None
        }
    }

    pub fn init(active_table: &mut ActivePageTable) -> Result<(), &'static str> {
        let fadt_sdt = find_matching_sdts("FACP");
        let fadt = if fadt_sdt.len() == 1 {
            load_table(get_sdt_signature(fadt_sdt[0]));
            Fadt::new(fadt_sdt[0])
        } else {
            error!("Unable to find FADT");
            return Err("Couldn't find FADT");
        };

        if let Some(fadt) = fadt {
            debug!("  FACP: {:X}  {:?}", fadt.dsdt, fadt);

            let dsdt_sdt = try!(get_sdt(fadt.dsdt as usize, active_table));

            let signature = get_sdt_signature(dsdt_sdt);
            if let Some(ref mut ptrs) = *(SDT_POINTERS.write()) {
                ptrs.insert(signature, dsdt_sdt);
            }

            let mut fadt_t = ACPI_TABLE.fadt.write();
            *fadt_t = Some(fadt);
            
            Ok(())
        }
        else {
            error!("Unable to find FADT");
            return Err("Couldn't find FADT");
        }
    }
}
