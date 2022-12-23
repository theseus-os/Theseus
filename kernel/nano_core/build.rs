use std::{env, io::Write, path::PathBuf, process::Command};

/// Whether or not to use the `built` crate to emit the default `built.rs` file.
const EMIT_BUILT_RS_FILE: bool = false;

/// The prefix that all custom rustc-known cfg keys are given by cargo
/// when it transforms them into environment variables.
const CARGO_CFG_PREFIX: &str = "CARGO_CFG_";

const BOOT_SPECIFICATION: &str = "bios";

/// The set of built-in environment variables defined by cargo.
static NON_CUSTOM_CFGS: [&str; 12] = [
    "CARGO_CFG_PANIC",
    "CARGO_CFG_TARGET_ABI",
    "CARGO_CFG_TARGET_ARCH",
    "CARGO_CFG_TARGET_ENDIAN",
    "CARGO_CFG_TARGET_ENV",
    "CARGO_CFG_TARGET_FEATURE",
    "CARGO_CFG_TARGET_HAS_ATOMIC",
    "CARGO_CFG_TARGET_HAS_ATOMIC_EQUAL_ALIGNMENT",
    "CARGO_CFG_TARGET_HAS_ATOMIC_LOAD_STORE",
    "CARGO_CFG_TARGET_OS",
    "CARGO_CFG_TARGET_POINTER_WIDTH",
    "CARGO_CFG_TARGET_VENDOR",
];

fn main() {
    emit_built_rs_file();
    compile_asm();
}

fn emit_built_rs_file() {
    // Note: we currently don't care about anything in the default `built.rs` file.
    // if EMIT_BUILT_RS_FILE {
    //     built::write_built_file().expect("The `built` crate failed to acquire build-time information");
    // }

    // Append our custom content to the initial `built.rs` file, if one exists.
    let built_rs_path = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("built.rs");
    let mut built_file = std::fs::File::options()
        .write(true)
        .create(true)
        .append(EMIT_BUILT_RS_FILE)
        .truncate(!EMIT_BUILT_RS_FILE)
        .open(built_rs_path)
        .expect("Failed to open the build-time information file");

    built_file.write_all(
        b"// BELOW: THESEUS-SPECIFIC BUILD INFORMATION THAT WAS AUTO-GENERATED DURING COMPILATION. DO NOT MODIFY.\n"
    ).expect("Failed to write to the build-time information file.");

    let mut num_custom_cfgs = 0usize;
    let mut custom_cfgs = String::new();
    let mut custom_cfgs_str = String::new();

    for (k, v) in std::env::vars() {
        if k.starts_with("CARGO_CFG_") && !NON_CUSTOM_CFGS.contains(&k.as_str()) {
            let key = k[CARGO_CFG_PREFIX.len()..].to_lowercase();
            custom_cfgs = format!("{}(\"{}\", \"{}\"), ", custom_cfgs, key, v);

            custom_cfgs_str.push_str(&key);
            if !v.is_empty() {
                custom_cfgs_str.push_str(&format!("=\"{}\"", v));
            }
            custom_cfgs_str.push(' ');

            num_custom_cfgs += 1;
        }
    }

    // Append all of the custom cfg values to the built.rs file as an array.
    write!(
        &mut built_file,
        "#[allow(dead_code)]\npub const CUSTOM_CFG: [(&str, &str); {}] = [{}];\n",
        num_custom_cfgs,
        custom_cfgs,
    ).unwrap();

    // Append all of the custom cfg values to the built.rs file as a single string.
    write!(
        &mut built_file,
        "#[allow(dead_code)]\npub const CUSTOM_CFG_STR: &str = r#\"{}\"#;\n",
        custom_cfgs_str,
    ).unwrap();
}

fn compile_asm() {
    let out_dir = match env::var("THESEUS_NANO_CORE_BUILD_DIR") {
        Ok(out_dir) => PathBuf::from(out_dir),
        // nano core is being compiled for docs or clippy
        Err(_) => std::env::temp_dir(),
    }
    .join("compiled_asm")
    .join(BOOT_SPECIFICATION);
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        if e.kind() != std::io::ErrorKind::AlreadyExists {
            panic!("failed to create compiled_asm directory: {}", e);
        }
    }
    let include_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("src")
        .join("asm");

    println!("cargo:rerun-if-changed={}", include_path.display());
    // Note: the below line causes the assembly files to be re-compiled on every nano_core build.
    //       This is currently favored because it is fast and avoids the need for a `make clean`.
    println!("cargo:rerun-if-changed={}", out_dir.display());

    let asm_path = include_path.join(BOOT_SPECIFICATION);

    let mut cflags = env::var("THESEUS_CFLAGS").unwrap_or_default();
    if BOOT_SPECIFICATION == "bios" {
        cflags.push_str(" -DBIOS");
    }


    for file in include_path
        .read_dir()
        .unwrap_or_else(|_| panic!("failed to open dir: {}", include_path.display()))
        .chain(
            asm_path.read_dir().unwrap_or_else(|_| panic!("failed to open asm dir: {}", asm_path.display()))
        )
    {
        let file = file.expect("failed to read asm file");
        if file
            .file_type()
            .unwrap_or_else(|_| panic!("couldn't get file type of {:?}", file))
            .is_file()
        {
            assert_eq!(file.path().extension(), Some("asm".as_ref()),
                "File {:?} did not have '.asm' extension", file.path(),
            );

            let mut output_path = out_dir.join(file.path().file_name().unwrap());
            assert!(output_path.set_extension("o"));

            let mut command = Command::new("nasm");
            command
                .args(["-f", "elf64"])
                .arg("-i")
                .arg(&include_path)
                .arg("-o")
                .arg(&output_path)
                .arg(file.path())
                .args(cflags.split(' '));

            assert!(command
                .status()
                .expect("failed to acquire nasm output status")
                .success());
        }
    }
}
