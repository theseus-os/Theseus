use std::{env, path::PathBuf, process::Command};

fn main() {
    compile_asm();

    let linker_file = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("linker.ld");
    println!("cargo:rustc-link-arg=-T{}", linker_file.display());
}

fn compile_asm() {
    let include_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("src")
        .join("asm");

    #[cfg(feature = "uefi")]
    let asm_path = include_path.join("uefi");
    #[cfg(feature = "bios")]
    let asm_path = include_path.join("bios");

    for file in include_path
        .read_dir()
        .expect("failed to open include directory")
        .chain(asm_path.read_dir().expect("failed to open asm directory"))
    {
        let file = file.expect("failed to read asm file");
        if file.file_type().expect("couldn't get file type").is_file() {
            let out_dir: PathBuf = env::var("OUT_DIR").unwrap().into();
            assert_eq!(file.path().extension(), Some("asm".as_ref()));

            let mut output_path = out_dir.join(file.path().file_name().unwrap());
            assert!(output_path.set_extension("o"));

            assert!(Command::new("nasm")
                .args(["-f", "elf64"])
                .arg("-i")
                .arg(&include_path)
                .arg("-o")
                .arg(&output_path)
                .arg(file.path())
                .status()
                .expect("failed to acquire nasm output status")
                .success());

            println!("cargo:rustc-link-arg={}", output_path.display());
        }
    }
}
