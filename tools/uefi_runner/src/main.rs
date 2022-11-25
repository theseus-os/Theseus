use std::path::PathBuf;

fn main() {
    let mut args = std::env::args();
    let kernel: PathBuf = args.nth(1).expect("kernel path not provided").into();
    let output: PathBuf = args.next().expect("output path not provided").into();

    bootloader::UefiBoot::new(&kernel)
        .create_disk_image(&output)
        .expect("failed to create uefi disk image");

    assert!(std::process::Command::new("qemu-system-x86_64")
        .args(["-bios".as_ref(), ovmf_prebuilt::ovmf_pure_efi().as_os_str()])
        .args(["-drive", &format!("format=raw,file={}", output.display())])
        // TODO: Make monitor configurable.
        .args(["-monitor", &format!("telnet:localhost:1235,server,nowait")])
        .status()
        .expect("failed to acquire qemu output status")
        .success());
}
