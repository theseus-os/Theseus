/// The virtual address where the initial kernel (the nano_core) is mapped to on aarch64.
/// Actual value: 0xFFFFFFFF00000000.
/// The UEFI firmware QEMU_EFI.fd loads kernel to the physical address 0x9C049000. If the kernel offset is 0xFFFF_FFFF_8000_0000 as for x86_64, the virtual address where the kernel is mapped to will overflow. An offset of 0xFFFF_FFFF_0000_0000 guarantees that the virtual address won't exceed the max address.
pub const KERNEL_OFFSET: usize = 0xFFFF_FFFF_0000_0000;
/// For higher half virtual address the bits from KERNEL_OFFSET_BITS_START to 64 are 1
pub const KERNEL_OFFSET_BITS_START: u8 = 48;
/// The prefix of higher half virtual address;
pub const KERNEL_OFFSET_PREFIX: usize = 0b1111_1111_1111_1111;

// Hardware resources https://github.com/qemu/qemu/blob/master/hw/arm/virt.c 
//     Hardware Resource            start address  size
//     [VIRT_FLASH] =              {          0, 0x08000000 },
//     [VIRT_CPUPERIPHS] =         { 0x08000000, 0x00020000 },
//     /* GIC distributor and CPU interfaces sit inside the CPU peripheral space */
//     [VIRT_GIC_DIST] =           { 0x08000000, 0x00010000 },
//     [VIRT_GIC_CPU] =            { 0x08010000, 0x00010000 },
//     [VIRT_GIC_V2M] =            { 0x08020000, 0x00001000 },
//     [VIRT_GIC_HYP] =            { 0x08030000, 0x00010000 },
//     [VIRT_GIC_VCPU] =           { 0x08040000, 0x00010000 },
//     /* The space in between here is reserved for GICv3 CPU/vCPU/HYP */
//     [VIRT_GIC_ITS] =            { 0x08080000, 0x00020000 },
//     /* This redistributor space allows up to 2*64kB*123 CPUs */
//     [VIRT_GIC_REDIST] =         { 0x080A0000, 0x00F60000 },
//     [VIRT_UART] =               { 0x09000000, 0x00001000 },
//     [VIRT_RTC] =                { 0x09010000, 0x00001000 },
//     [VIRT_FW_CFG] =             { 0x09020000, 0x00000018 },
//     [VIRT_GPIO] =               { 0x09030000, 0x00001000 },
//     [VIRT_SECURE_UART] =        { 0x09040000, 0x00001000 },
//     [VIRT_SMMU] =               { 0x09050000, 0x00020000 },
//     [VIRT_MMIO] =               { 0x0a000000, 0x00000200 },
//     /* ...repeating for a total of NUM_VIRTIO_TRANSPORTS, each of that size */
//     [VIRT_PLATFORM_BUS] =       { 0x0c000000, 0x02000000 },
//     [VIRT_SECURE_MEM] =         { 0x0e000000, 0x01000000 },
//     [VIRT_PCIE_MMIO] =          { 0x10000000, 0x2eff0000 },
//     [VIRT_PCIE_PIO] =           { 0x3eff0000, 0x00010000 },
//     [VIRT_PCIE_ECAM] =          { 0x3f000000, 0x01000000 },
pub const HARDWARE_START: u64 = 0x1000;
pub const HARDWARE_END: u64 = 0x40000000;