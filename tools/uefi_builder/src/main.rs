use std::path::PathBuf;

fn main() {
    let mut args = std::env::args();
    let kernel: PathBuf = args.nth(1).expect("kernel input path not provided").into();
    let modules: PathBuf = args.next().expect("modules input path not provided").into();
    let efi_image: PathBuf = args
        .next()
        .expect("efi image output path not provided")
        .into();
    let efi_firmware: PathBuf = args
        .next()
        .expect("uefi firmware output path not provided")
        .into();

    let mut bootloader = bootloader::UefiBoot::new(&kernel);

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

    bootloader
        .create_disk_image(&efi_image)
        .expect("failed to create uefi disk image");

    std::fs::copy(ovmf_prebuilt::ovmf_pure_efi(), efi_firmware)
        .expect("couldn't copy efi firmware");
}
