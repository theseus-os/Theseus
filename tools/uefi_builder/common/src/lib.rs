mod fs;

use clap::Parser;
use fs::UefiBoot;
use std::path::{Path, PathBuf};

#[derive(Parser)]
struct Args {
    /// Path to the kernel image.
    #[arg(long)]
    kernel: PathBuf,
    /// Path to the modules directory.
    #[arg(long)]
    modules: Option<PathBuf>,
    /// Path at which the EFI image should be placed.
    #[arg(long)]
    efi_image: PathBuf,
    /// Path at which the EFI firmware should be placed.
    #[arg(long)]
    efi_firmware: Option<PathBuf>,
}

pub fn main(bootloader_host_path: &Path, bootloader_efi_path: &Path) {
    let Args {
        kernel,
        modules,
        efi_image,
        efi_firmware,
    } = Args::parse();

    let mut bootloader = UefiBoot::new(&kernel);

    if let Some(modules) = modules {
        for file in modules
            .read_dir()
            .expect("failed to open modules directory")
        {
            let file = file.expect("failed to read file");
            if file.file_type().expect("couldn't get file type").is_file() {
                bootloader.add_file(
                    format!(
                        "modules/{}",
                        file.file_name()
                            .to_str()
                            .expect("couldn't convert path to str")
                    )
                    .into(),
                    file.path(),
                );
            }
        }
    }

    bootloader
        .create_disk_image(bootloader_host_path, bootloader_efi_path, &efi_image)
        .expect("failed to create uefi disk image");

    if let Some(efi_firmware) = efi_firmware {
        std::fs::copy(ovmf_prebuilt::ovmf_pure_efi(), efi_firmware)
            .expect("couldn't copy efi firmware");
    }
}
