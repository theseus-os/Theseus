use std::path::PathBuf;

fn main() {
    let mut args = std::env::args();
    let kernel: PathBuf = args.nth(1).expect("kernel path not provided").into();
    let output: PathBuf = args.next().expect("output path not provided").into();

    bootloader::UefiBoot::new(&kernel)
        .create_disk_image(&output)
        .expect("failed to create uefi disk image");

    let mut cmd = std::process::Command::new("qemu-system-x86_64")
        .args(["-bios", ovmf_prebuilt::ovmf_pure_efi()])
        .args(["-drive", format!("format=raw,file={uefi_path}")])
        // TODO: Make monitor configurable.
        .args(["-monitor", format!("telnet:localhost:1235,server,nowait")]);
}
