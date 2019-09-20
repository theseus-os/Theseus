//! The build script that is used to specify which conditional compilation 
//! options should be enabled when building Theseus.  

/// The prefix that must come before each custom cfg option.
const CFG_PREFIX: &'static str = "cargo:rustc-cfg=";


// const APIC_TIMER_FIXED: &'static str = "apic_timer_fixed";
// const LOADABLE: &'static str = "loadable";
// const MIRROR_LOG_TO_VGA: &'static str = "mirror_log_to_vga";
// const SIMD_PERSONALITY: &'static str = "simd_personality";
// const PRIORITY_SCHEDULER: &'static str = "priority_scheduler";

fn main() {
    println!("cargo:rerun-if-env-changed=THESEUS_CONFIG");
    let configs = std::env::var("THESEUS_CONFIG").unwrap_or(String::new());
    
	// here we just need to print out every provided config option
    for s in configs.split_whitespace() {
        println!("{}{}", CFG_PREFIX, s);
    }

    // println!("cargo:rustc-link-search=static_libs/");
    // println!("cargo:rustc-link-lib=static=test");

    eprintln!("ran build script, configs: {}", configs);

}



// fn loadable() -> Vec<String> {
//     vec![
//         LOADABLE,
//     ]
// }


// fn bochs<S>() -> Vec<S> where S: Into<String>
//     vec![
//         APIC_TIMER_FIXED,
//     ]
// }


