extern crate cc;

use std::{env, fs, fs::DirEntry}; //, path::Path};

// // include src/header directories that don't start with '_'
// fn include_dir(d: &DirEntry) -> bool {
//     d.metadata().map(|m| m.is_dir()).unwrap_or(false)
//         && d.path()
//             .iter()
//             .nth(2)
//             .map_or(false, |c| c.to_str().map_or(false, |x| !x.starts_with("_")))
// }

// fn generate_bindings(cbindgen_config_path: &Path) {
//     let relative_path = cbindgen_config_path
//         .strip_prefix("src/header")
//         .ok()
//         .and_then(|p| p.parent())
//         .and_then(|p| p.to_str())
//         .unwrap()
//         .replace("_", "/");
//     let header_path = Path::new("target/include")
//         .join(&relative_path)
//         .with_extension("h");
//     let mod_path = cbindgen_config_path.with_file_name("mod.rs");
//     let config = cbindgen::Config::from_file(cbindgen_config_path).unwrap();
//     cbindgen::Builder::new()
//         .with_config(config)
//         .with_src(mod_path)
//         .generate()
//         .expect("Unable to generate bindings")
//         .write_to_file(header_path);
// }

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");

    eprintln!("crate_dir: {:?}", crate_dir);

    let mut c_build = cc::Build::new();
    c_build
        .no_default_flags(true)
        .flag("-nostdinc")
        .flag("-nostdlib")
        .flag("-nostartfiles")
        .flag("-mno-red-zone")
        .flag("-mcmodel=large")
        .flag("-static-libgcc")
        .flag("-z max-page-size=4096")

        // Theseus's loader/linker expects all sections to be kept separate
        // or merged by its own partial relinking script.
        .flag("-ffunction-sections")
        .flag("-fdata-sections")

        // disable PLT, Procedure Linkage Table, a type of relocation entry Theseus doesn't yet support
        .flag("-fno-plt")
        .use_plt(false)

        // disable PIC, which disables usage of the GOT (Global Offset Table), a type of relocation entry Theseus doesn't yet support
        .flag("-fno-pic") 
        .pic(false)

        .static_flag(false)

        .include(&format!("{}/include", crate_dir))
        .flag("-fno-stack-protector")
        .flag("-Wno-expansion-to-defined")
        .files(
            fs::read_dir("src/c_wrappers")
                .expect("src/c_wrappers directory missing")
                .map(|res| res.expect("read_dir error").path()),
        );

    eprintln!("c_build: {:#?}", c_build);
    eprintln!("c_build tool: {:#?}", c_build.get_compiler());
        
    c_build.compile("tlibc_c");

}
