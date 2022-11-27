use std::path::PathBuf;

fn main() {
    let mut args = std::env::args();
    let kernel: PathBuf = args.nth(1).expect("kernel path not provided").into();
    let efi_image: PathBuf = args.next().expect("efi image output path not provided").into();
    let efi_firmware: PathBuf = args.next().expect("uefi firmware output path not provided").into();

    bootloader::UefiBoot::new(&kernel)
        .create_disk_image(&efi_image)
        .expect("failed to create uefi disk image");
    
    std::fs::copy(ovmf_prebuilt::ovmf_pure_efi(), efi_firmware).expect("couldn't copy efi firmware");

    // assert!(std::process::Command::new("qemu-system-x86_64")
    //     .args(["-bios".as_ref(), ovmf_prebuilt::ovmf_pure_efi().as_os_str()])
    //     .args(["-drive", &format!("format=raw,file={}", output.display())])
    //     // TODO: Make monitor configurable.
    //     .args(["-monitor", &format!("telnet:localhost:1235,server,nowait")])
    //     .status()
    //     .expect("failed to acquire qemu output status")
    //     .success());
}
