use frame_allocator::MemoryRegionType::{self, *};
use uefi::table::boot::MemoryType;

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
