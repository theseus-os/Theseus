use std::path::Path;

fn main() {
    uefi_builder_common::main(
        Path::new(env!("CARGO_BIN_FILE_UEFI_BOOTLOADER")),
        Path::new("efi/boot/bootaa64.efi"),
    );
}
