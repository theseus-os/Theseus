use std::io::Write;

/// Whether or not to use the `built` crate to emit the default `built.rs` file.
const EMIT_BUILT_RS_FILE: bool = false;

/// The prefix that all custom rustc-known cfg keys are given by cargo
/// when it transforms them into environment variables.
const CARGO_CFG_PREFIX: &'static str = "CARGO_CFG_";

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
            let key = k[CARGO_CFG_PREFIX.len() ..].to_lowercase();
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
