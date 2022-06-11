//! This build script is used to enable the `frame_pointers` cfg option
//! if the corresponding rustflags value is set.

/// The prefix that must come before each custom cfg option.
const CFG_PREFIX: &'static str = "cargo:rustc-cfg=";


fn main() {
    println!("cargo:rerun-if-env-changed=THESEUS_CONFIG");
    println!("cargo:rerun-if-env-changed=CARGO_ENCODED_RUSTFLAGS");

    // Since there is no known way to obtain cfg values for codegen options,
    // we must set the cfg value for force-frame-pointers.
    if let Ok(rustflags) = std::env::var("CARGO_ENCODED_RUSTFLAGS") {
        if rustflags.contains("force-frame-pointers=yes")
        || rustflags.contains("force-frame-pointers=true") {
            println!("{}{}", CFG_PREFIX, "frame_pointers");
        }
    } else {
        eprintln!("Note: CARGO_ENCODED_RUSTFLAGS env var did not exist.");
    }
}
