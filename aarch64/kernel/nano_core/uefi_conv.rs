use frame_allocator::MemoryRegionType::{self, *};
use uefi::table::boot::MemoryType;
use pte_flags::PteFlags;

pub fn convert_mem(uefi: MemoryType) -> MemoryRegionType {
    match uefi {
        // This enum variant is not used.
        MemoryType::RESERVED              => Unknown,

        // The code portions of a loaded UEFI application.
        MemoryType::LOADER_CODE           => Reserved,

        // The data portions of a loaded UEFI applications,
        // as well as any memory allocated by it.
        MemoryType::LOADER_DATA           => Reserved,

        // Code of the boot drivers.
        // Can be reused after OS is loaded.
        MemoryType::BOOT_SERVICES_CODE    => Reserved,

        // Memory used to store boot drivers' data.
        // Can be reused after OS is loaded.
        MemoryType::BOOT_SERVICES_DATA    => Reserved,

        // Runtime drivers' code.
        // Nathan: Again, we don't know...
        // Do you think we can expect this to be normal mem?
        MemoryType::RUNTIME_SERVICES_CODE => Reserved,

        // Runtime services' code.
        MemoryType::RUNTIME_SERVICES_DATA => Reserved,

        // Free usable memory.
        MemoryType::CONVENTIONAL          => Free,

        // Memory in which errors have been detected.
        MemoryType::UNUSABLE              => Unknown,

        // Memory that holds ACPI tables.
        // Can be reclaimed after they are parsed.
        MemoryType::ACPI_RECLAIM          => Reserved,

        // Firmware-reserved addresses.
        MemoryType::ACPI_NON_VOLATILE     => Reserved,

        // A region used for memory-mapped I/O.
        MemoryType::MMIO                  => Reserved,

        // Address space used for memory-mapped port I/O.
        MemoryType::MMIO_PORT_SPACE       => Reserved,

        // Address space which is part of the processor.
        MemoryType::PAL_CODE              => Reserved,

        // Memory region which is usable and is also non-volatile.
        MemoryType::PERSISTENT_MEMORY     => Reserved,

        _ => Unknown,
    }
}

pub fn get_mem_flags(uefi: MemoryType) -> Option<PteFlags> {
    match uefi {
        // This enum variant is not used.
        MemoryType::RESERVED              => None,

        // The code portions of a loaded UEFI application.
        // We get permission faults if this isn't writable
        MemoryType::LOADER_CODE           => Some(PteFlags::WRITABLE),

        // The data portions of a loaded UEFI applications,
        // as well as any memory allocated by it.
        MemoryType::LOADER_DATA           => Some(PteFlags::NOT_EXECUTABLE | PteFlags::WRITABLE),

        // Code of the boot drivers.
        // Can be reused after OS is loaded.
        // We get permission faults if this isn't writable
        MemoryType::BOOT_SERVICES_CODE    => Some(PteFlags::WRITABLE),

        // Memory used to store boot drivers' data.
        // Can be reused after OS is loaded.
        MemoryType::BOOT_SERVICES_DATA    => Some(PteFlags::NOT_EXECUTABLE | PteFlags::WRITABLE),

        // Runtime drivers' code.
        // We get permission faults if this isn't writable
        MemoryType::RUNTIME_SERVICES_CODE => Some(PteFlags::WRITABLE),

        // Runtime services' code.
        MemoryType::RUNTIME_SERVICES_DATA => Some(PteFlags::NOT_EXECUTABLE | PteFlags::WRITABLE),

        // Free usable memory.
        MemoryType::CONVENTIONAL          => None,

        // Memory in which errors have been detected.
        MemoryType::UNUSABLE              => None,

        // Memory that holds ACPI tables.
        // Can be reclaimed after they are parsed.
        MemoryType::ACPI_RECLAIM          => Some(PteFlags::DEVICE_MEMORY | PteFlags::NOT_EXECUTABLE | PteFlags::WRITABLE),

        // Firmware-reserved addresses.
        MemoryType::ACPI_NON_VOLATILE     => Some(PteFlags::DEVICE_MEMORY | PteFlags::NOT_EXECUTABLE | PteFlags::WRITABLE),

        // A region used for memory-mapped I/O.
        MemoryType::MMIO                  => Some(PteFlags::DEVICE_MEMORY | PteFlags::NOT_EXECUTABLE | PteFlags::WRITABLE),

        // Address space used for memory-mapped port I/O.
        MemoryType::MMIO_PORT_SPACE       => Some(PteFlags::DEVICE_MEMORY | PteFlags::NOT_EXECUTABLE | PteFlags::WRITABLE),

        // Address space which is part of the processor.
        MemoryType::PAL_CODE              => Some(PteFlags::DEVICE_MEMORY | PteFlags::NOT_EXECUTABLE | PteFlags::WRITABLE),

        // Memory region which is usable and is also non-volatile.
        MemoryType::PERSISTENT_MEMORY     => Some(PteFlags::DEVICE_MEMORY | PteFlags::NOT_EXECUTABLE | PteFlags::WRITABLE),

        _ => None,
    }
}
