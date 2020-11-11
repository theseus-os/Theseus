//! Traverses the Rust output directory of compiled crate object files
//! and copies the latest version of each one into the OS ISO image directory.
//! 
//! This procedure is required to properly implement incremental builds, 
//! in which multiple versions of a given crate end up being built into the single target directory. 
//! While it may be valid to have multiple different versions of a third-party crate object file
//! present in the final OS image, 
//! it is *not* valid to have multiple different versions of a first-party Theseus crate object file
//! in the final OS image 
//! (at least not in the default set of modules used to boot the OS, ignoring live evolution cases).
//! For example, it's legal to have something like `log v0.3.7` and `log v0.4.0` co-exist in 
//! the final OS image, in which they would each have different hash suffixes. 
//! But it's not legal to have something like `captain-<hash1>` and `captain-<hash2>` in the OS image,
//! since there should only be one.
//! 
//! Thus, this application selects the latest version of each first-party Theseus crate, 
//! both for applications and kernel crates, and copies it into the OS image directory.
//! For third-party crates, *all* instances are copied into the OS image directory. 
//! Currently, **library** crates (in the `./libs/` directory) are treated as third-party crates.
//! 
//! By default, the `target` directory holds those object files,
//! and we generally want to place them into `build/grub-isofiles/modules/`. 
//! These directories should be passed in to this executable as command-line arguments. 
//! 

extern crate getopts;
extern crate walkdir;

use getopts::Options;
use std::{
    collections::{
        HashSet,
        HashMap,
        hash_map::Entry,
    },
    env,
    fs::{self, DirEntry, File},
    io::{self, BufRead},
    path::Path,
};
use walkdir::WalkDir;


/// Debug option: if true, print all crate names and their object file path. 
const PRINT_CRATES: bool = true;
/// Debug option: if both this and `PRINT_CRATES` are true, print sorted crate names. 
const PRINT_SORTED: bool = true;


/// The delimiter that goes at the end of object file prefixes,
/// between the prefix and the remainder of the crate name/hash.
/// For example, "k#my_crate-hash.o".
pub const MODULE_PREFIX_DELIMITER: &'static str = "#";


fn main() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();

    let mut opts = Options::new();
    opts.reqopt(
        "i", 
        "input",  
        "path to the input directory of compiled crate object files, 
         typically the `target`, e.g., \"/path/to/Theseus/target\"", 
        "TARGET_DIR"
    );
    opts.reqopt(
        "o", 
        "output",  
        "path to the output directory where crate object files should be copied to, 
         typically the OS image directory, e.g., \"/path/to/build/grub-isofiles/modules/\"", 
        "OUTPUT_DIR"
    );
    opts.reqopt(
        "k", 
        "kernel",  
        "path to either the directory of kernel crates, or a file listing each kernel crate name, one per line",
        "KERNEL_CRATES"
    );
    opts.reqopt(
        "a", 
        "app",  
        "path to either the directory of application crates, or a file listing each application crate name, one per line",
        "APP_CRATES"
    );
    opts.optopt(
        "",
        "--kernel-prefix",
        "the prefix prepended to kernel crate object files when they're copied to the output directory (default: 'k#')",
        "PREFIX"
    );
    opts.optopt(
        "",
        "--app-prefix",
        "the prefix prepended to application crate object files when they're copied to the output directory (default 'a#')",
        "PREFIX"
    );
    opts.optflag("h", "help", "print this help menu");

    let matches = opts.parse(&args[1..]).map_err(|e| e.to_string())?;

    if matches.opt_present("h") {
        print_usage("cargo run -- ", opts);
        return Ok(());
    }

    let mut kernel_prefix = matches.opt_str("--kernel-prefix").unwrap_or("k".to_string());
    if !kernel_prefix.ends_with(MODULE_PREFIX_DELIMITER) {
        kernel_prefix.push_str(MODULE_PREFIX_DELIMITER);
    }
    let mut app_prefix = matches.opt_str("--app-prefix").unwrap_or("a".to_string());
    if !app_prefix.ends_with(MODULE_PREFIX_DELIMITER) {
        app_prefix.push_str(MODULE_PREFIX_DELIMITER);
    }

    let kernel_arg = matches.opt_str("k").expect("no -k or --kernel arg provided");
    let app_arg    = matches.opt_str("a").expect("no -a or --app arg provided");
    let input_dir  = matches.opt_str("i").expect("no -i or --input arg provided");
    let output_dir = matches.opt_str("o").expect("no -o or --output arg provided");

    let kernel_arg_path = fs::canonicalize(kernel_arg)
        .map_err(|e| format!("kernel arg was invalid path, error: {:?}", e))?;
    let kernel_crates_set = if kernel_arg_path.is_file() {
        populate_crates_from_file(kernel_arg_path)
            .map_err(|e| format!("Error parsing kernel arg as file: {:?}", e))?
    } else if kernel_arg_path.is_dir() {
        populate_crates_from_dir(kernel_arg_path)
            .map_err(|e| format!("Error parsing kernel arg as directory: {:?}", e))?
    } else {
        return Err(format!("Couldn't access -k/--kernel argument {:?} as a file or directory", kernel_arg_path));
    };

    let app_arg_path = fs::canonicalize(app_arg)
        .map_err(|e| format!("app arg was invalid path, error: {:?}", e))?;
    let app_crates_set = if app_arg_path.is_file() {
        populate_crates_from_file(app_arg_path)
            .map_err(|e| format!("Error parsing app arg as file: {:?}", e))?
    } else if app_arg_path.is_dir() {
        populate_crates_from_dir(app_arg_path)
            .map_err(|e| format!("Error parsing app arg as directory: {:?}", e))?
    } else {
        return Err(format!("Couldn't access -k/--app argument {:?} as a file or directory", app_arg_path));
    };

    let (kernel_object_files, app_object_files, other_object_files) = parse_input_dir(
        kernel_crates_set,
        app_crates_set,
        input_dir,
    ).unwrap();

    // Now that we have obtained the lists of kernel, app, and other crates, 
    // we copy them into the output directory with the proper prefix. 
    copy_files(&output_dir, kernel_object_files, &kernel_prefix).unwrap();
    copy_files(&output_dir, app_object_files,    &app_prefix   ).unwrap();
    copy_files(&output_dir, other_object_files,  &kernel_prefix).unwrap(); 
    

    
    // // Write to output file (if provided) 
    // if matches.opt_present("o") {
    //     let output_file_path = matches.opt_str("o")
    //         .ok_or_else(|| String::from("failed to match output file argument."))?;
    //     write_content(grub_cfg_string, output_file_path);
    // }
    // // Otherwise, write to stdout by default
    // else {
    //     println!("{}", grub_cfg_string);
    // }

    Ok(())
}

fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} [options]", program);
    print!("{}", opts.usage(&brief));
}

/// Parses the file as a list of crate names, one per line.
/// 
/// Returns the set of unique crate names. 
fn populate_crates_from_file<P: AsRef<Path>>(file_path: P) -> Result<HashSet<String>, io::Error> {
    let file = File::open(file_path)?;
    let mut crates: HashSet<String> = HashSet::new();
    for line in io::BufReader::new(file).lines() {
        if let Some(crate_name) = line?.split("-").next() {
            crates.insert(crate_name.to_string());
        }
    }

    Ok(crates)
}

/// Iterates over the contents of the given directory to find crates within it. 
/// 
/// Crates are discovered by looking for a directory that contains a `Cargo.toml` file. 
/// 
/// Returns the set of unique crate names. 
fn populate_crates_from_dir<P: AsRef<Path>>(dir_path: P) -> Result<HashSet<String>, io::Error> {
    let mut crates: HashSet<String> = HashSet::new();
    
    let dir_iter = WalkDir::new(dir_path)
        .into_iter()
        .filter_map(|res| res.ok());
        // .filter(|entry| entry.path().is_file() && entry.path().extension() == Some(object_file_extension))
        // .filter_map(|entry| entry.path().file_name()
        //     .map(|fname| {
        //         (
        //             fname.to_string_lossy().to_string().into(), 
        //             entry.path().to_path_buf()
        //         )
        //     })
        // );

    for dir_entry in dir_iter {
        if dir_entry.file_type().is_file() && dir_entry.file_name() == "Cargo.toml" {
            // the parent of this dir_entry is a crate directory
            let parent_crate_dir = dir_entry.path().parent().ok_or_else(|| {
                let err_str = format!("Error getting the containing (parent) crate directory of a Cargo.toml file: {:?}", dir_entry.path());
                io::Error::new(io::ErrorKind::NotFound, err_str)
            })?;
            let parent_crate_name = parent_crate_dir.file_name().ok_or_else(|| {
                let err_str = format!("Error getting the name of crate directory {:?}", parent_crate_dir);
                io::Error::new(io::ErrorKind::NotFound, err_str)
            })?;
            crates.insert(parent_crate_name.to_str().unwrap().to_string());
        }

    }
    Ok(crates)
}


/// Parses the given input directory, which should be the directory of object files built by Rust, 
/// to determine the latest versions of kernel crates, application crates, and other crates.
/// 
/// See the top of this file for more details. 
/// 
/// Upon success, returns a tuple of:
/// * kernel crates
/// * application crates
/// * all other crates
/// 
fn parse_input_dir(
    kernel_crates: HashSet<String>,
    app_crates: HashSet<String>,
    input_dir: String,
) -> std::io::Result<(HashMap<String, DirEntry>, HashMap<String, DirEntry>, HashMap<String, DirEntry>)> {

    let mut kernel_objects:  HashMap<String, DirEntry> = HashMap::new();
    let mut app_objects:     HashMap<String, DirEntry> = HashMap::new();
    let mut other_objects:   HashMap<String, DirEntry> = HashMap::new();

    for dir_entry in fs::read_dir(input_dir)? {
        let dir_entry = dir_entry?;
        let metadata = dir_entry.metadata()?;
        if !metadata.is_file() { continue; }
        let file_name = dir_entry.file_name().into_string().unwrap();
        if !file_name.ends_with(".o") { continue; }
        let file_stem = file_name.split(".o").next().expect("object file name didn't have the .o extension");
        let prefix = file_name.split("-").next().expect("object file name didn't have the crate/hash '-' delimiter");
        let modified_time = metadata.modified()?;


        if kernel_crates.contains(prefix) {
            match kernel_objects.entry(prefix.to_string()) {
                Entry::Occupied(mut occupied) => {
                    if occupied.get().metadata()?.modified()? < modified_time {
                        occupied.insert(dir_entry);
                    }
                }
                Entry::Vacant(vacant) => {
                    vacant.insert(dir_entry);
                }
            }
        } else if app_crates.contains(prefix) {
            match app_objects.entry(prefix.to_string()) {
                Entry::Occupied(mut occupied) => {
                    if occupied.get().metadata()?.modified()? < modified_time {
                        occupied.insert(dir_entry);
                    }
                }
                Entry::Vacant(vacant) => {
                    vacant.insert(dir_entry);
                }
            }
        } else {
            other_objects.insert(file_stem.to_string(), dir_entry);
        }

    }

    // optional debug output
    if PRINT_CRATES {
        println!("KERNEL OBJECTS:");
        print_crates(&kernel_objects, PRINT_SORTED);
        println!("APPLICATION OBJECTS:");
        print_crates(&app_objects, PRINT_SORTED);
        println!("OTHER OBJECTS:");
        print_crates(&other_objects, PRINT_SORTED);
    }
    
    Ok((
        kernel_objects,
        app_objects,
        other_objects,
    ))
}


fn copy_files<P: AsRef<Path>>(
    output_dir: P,
    objects: HashMap<String, DirEntry>,
    prefix: &str
) -> io::Result<()> {
    
    for (_source_crate, source_file) in objects {
        let mut dest_path = output_dir.as_ref().to_path_buf();
        dest_path.push(format!("{}{}", prefix, source_file.file_name().to_str().unwrap()));
        println!("Copying {} to {}", source_file.path().display(), dest_path.display());
        fs::copy(source_file.path(), dest_path)?;
    }

    Ok(())
}



fn print_crates(objects: &HashMap<String, DirEntry>, sorted: bool) {
    if sorted {
        let mut sorted = objects.keys().collect::<Vec<&String>>();
        sorted.sort_unstable();
        for o in &sorted {
            println!("\t{}", o);
        }
    } else {
        for (k, v) in objects.iter() {
            println!("\t{} --> {}", k, v.path().display());
        }
    }
}
