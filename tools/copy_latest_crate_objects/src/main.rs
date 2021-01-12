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
    path::{Path, PathBuf},
};
use walkdir::WalkDir;


/// Debug option: if true, print all crate names and their object file path. 
const PRINT_CRATES: bool = true;
/// Debug option: if both this and `print_crates_objects` are true, print sorted crate names. 
const PRINT_SORTED: bool = false;


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
        "(required) path to the input directory of compiled crate .rlib, .rmeta, and .o files, 
         typically the `target`, e.g., \"/path/to/Theseus/target/$TARGET/release/deps/\"", 
        "INPUT_DIR"
    );
    opts.reqopt(
        "", 
        "output-objects",  
        "(required) path to the output directory where crate object files should be copied to, 
         typically the OS image directory, e.g., \"/path/to/build/grub-isofiles/modules/\"", 
        "OUTPUT_DIR"
    );
    opts.reqopt(
        "", 
        "output-deps",  
        "(required) path to the output directory where crate .rmeta and .rlib files should be copied to, 
         typically part of the build directory, e.g., \"/path/to/build/deps/\"", 
        "OUTPUT_DIR"
    );
    opts.optopt(
        "", 
        "output-sysroot",  
        "path to the output directory where the sysroot files should be copied to,
         which includes the .rmeta and .rlib files for fundamental Rust library crates, e.g., core, alloc, compiler_builtins. 
         Typically this should be \"/path/to/build/deps/sysroot/lib/rustlib/$TARGET/lib/\".
         If not provided, no sysroot output directory will be created.",
        "OUTPUT_DIR"
     );
    opts.reqopt(
        "k", 
        "kernel",  
        "(required) path to either the directory of kernel crates,
         or a file listing each kernel crate name, one per line",
        "KERNEL_CRATES"
    );
    opts.reqopt(
        "a", 
        "app",  
        "(required) path to either the directory of application crates,
         or a file listing each application crate name, one per line",
        "APP_CRATES"
    );
    opts.optopt(
        "",
        "kernel-prefix",
        "the prefix prepended to kernel crate object files when they're copied to the output directory (default: 'k#')",
        "PREFIX"
    );
    opts.optopt(
        "",
        "app-prefix",
        "the prefix prepended to application crate object files when they're copied to the output directory (default 'a#')",
        "PREFIX"
    );
    opts.optmulti(
        "e",
        "extra-app",
        "additional names of crates that should be treated as application crates. Can be provided multiple times",
        "APP_CRATE_NAME"
    );
    opts.optflag("h", "help", "print this help menu");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(e) => {
            print_usage(opts);
            return Err(e.to_string());
        }
    };

    if matches.opt_present("h") {
        print_usage(opts);
        return Ok(());
    }

    let mut kernel_prefix = matches.opt_str("kernel-prefix").unwrap_or("k".to_string());
    if !kernel_prefix.ends_with(MODULE_PREFIX_DELIMITER) {
        kernel_prefix.push_str(MODULE_PREFIX_DELIMITER);
    }
    if kernel_prefix.matches(MODULE_PREFIX_DELIMITER).count() > 1 {
        return Err(format!("kernel-prefix {:?} must only contain one '#' character at the end!", kernel_prefix));
    }
    let mut app_prefix = matches.opt_str("app-prefix").unwrap_or("a".to_string());
    if !app_prefix.ends_with(MODULE_PREFIX_DELIMITER) {
        app_prefix.push_str(MODULE_PREFIX_DELIMITER);
    }
    if app_prefix.matches(MODULE_PREFIX_DELIMITER).count() > 1 {
        return Err(format!("app-prefix {:?} must only contain one '#' character at the end!", app_prefix));
    }

    // Parse the required command-line arguments.
    let input_dir          = matches.opt_str("i").expect("no -i or --input arg provided");
    let output_objects_dir = matches.opt_str("output-objects").expect("no --output-objects arg provided");
    let output_deps_dir    = matches.opt_str("output-deps").expect("no --output-deps arg provided");
    let kernel_arg         = matches.opt_str("k").expect("no -k or --kernel arg provided");
    let app_arg            = matches.opt_str("a").expect("no -a or --app arg provided");

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
    let mut app_crates_set = if app_arg_path.is_file() {
        populate_crates_from_file(app_arg_path)
            .map_err(|e| format!("Error parsing app arg as file: {:?}", e))?
    } else if app_arg_path.is_dir() {
        populate_crates_from_dir(app_arg_path)
            .map_err(|e| format!("Error parsing app arg as directory: {:?}", e))?
    } else {
        return Err(format!("Couldn't access -k/--app argument {:?} as a file or directory", app_arg_path));
    };

    let extra_app_names = matches.opt_strs("e");
    app_crates_set.extend(extra_app_names.iter().flat_map(|n| n.split_whitespace()).map(|s| s.to_string()));

    let (
        app_object_files,
        kernel_objects_and_deps_files,
        other_objects_and_deps_files,
    ) = parse_input_dir(
        app_crates_set,
        kernel_crates_set,
        input_dir,
    ).unwrap();


    // Now that we have obtained the lists of kernel, app, and other crates, 
    // we copy their crate object files into the output object directory with the proper prefix. 
    // Also, we ensure that the specified output directory exists.
    fs::create_dir_all(&output_objects_dir).map_err(|e| 
        format!("Error creating output objects directory {:?}, {:?}", output_objects_dir, e)
    )?;
    copy_files(
        &output_objects_dir,
        app_object_files.values().map(|d| d.path()),
        &app_prefix
    ).unwrap();
    copy_files(
        &output_objects_dir,
        kernel_objects_and_deps_files.values().map(|(obj_direnty, _)| obj_direnty.path()),
        &kernel_prefix
    ).unwrap();
    copy_files(
        &output_objects_dir,
        other_objects_and_deps_files.values().map(|(obj_direnty, _)| obj_direnty.path()),
        &kernel_prefix
    ).unwrap(); 


    
    // Now we do the same kind of copy operation of crate dependency files, namely the .rlib and .rmeta files,
    // into the output deps directory. 
    fs::create_dir_all(&output_deps_dir).map_err(|e|
        format!("Error creating output deps directory {:?}, {:?}", output_deps_dir, e)
    )?;
    copy_files(
        &output_deps_dir,
        kernel_objects_and_deps_files.values().flat_map(|(_, deps)| deps.iter()),
        "",
    ).unwrap();
    // Currently we also copy non-kernel dependency files just for efficiency in future out-of-tree builds.
    copy_files(
        &output_deps_dir,
        other_objects_and_deps_files.values().flat_map(|(_, deps)| deps.iter()),
        "",
    ).unwrap();

    // Here, if requested, we create the sysroot directory, which is inside of the output_deps directory. 
    // This will include the fundamental Rust libraries, e.g., core, alloc, compiler_builtins
    // that cargo has custom-built for our Theseus target.
    if let Some(output_sysroot_dir) = matches.opt_str("output-sysroot") {
        fs::create_dir_all(&output_sysroot_dir).map_err(|e|
            format!("Error creating output sysroot directory {:?}, {:?}", output_sysroot_dir, e)
        )?;
        let sysroot_files = other_objects_and_deps_files.iter()
            .filter(|(crate_name, val)| {
                crate_name.starts_with("core-") || 
                crate_name.starts_with("compiler_builtins-") || 
                crate_name.starts_with("rustc_std_workspace_core-") || 
                crate_name.starts_with("alloc-")
            })
            .flat_map(|(_key, (_, deps))| deps.iter());
        copy_files(
            &output_sysroot_dir,
            sysroot_files,
            "",
        ).unwrap();
    }

    Ok(())
}

fn print_usage(opts: Options) {
    let brief = format!("Usage: cargo run -- [options]");
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


/// A key-value set of crate dependency files, in which 
/// the key is the crate name, and the value is the crate's object file.
type CrateObjectFiles = HashMap<String, DirEntry>;
/// A key-value set of crate dependency files, in which 
/// the key is the crate name, and 
/// the value is a tuple of the crate's `(object file, [.rmeta file, .rlib file])`. 
type CrateObjectAndDepsFiles = HashMap<String, (DirEntry, [PathBuf; 2])>;


const DEPS_PREFIX:     &str = "lib";
const RMETA_EXTENSION: &str = "rmeta";
const RLIB_EXTENSION:  &str = "rlib";


/// Parses the given input directory, which should be the directory of object files built by Rust, 
/// to determine the latest versions of kernel crates, application crates, and other crates.
/// 
/// See the top of this file for more details. 
/// 
/// Upon success, returns a tuple of:
/// * application crate object files,
/// * kernel crate object files,
/// * all other crate object files,
/// * kernel dependency files (.rmeta and .rlib),
/// * all other non-application dependency files (.rmeta and .rlib).
/// 
fn parse_input_dir(
    app_crates: HashSet<String>,
    kernel_crates: HashSet<String>,
    input_dir: String,
) -> std::io::Result<(
    CrateObjectFiles,
    CrateObjectAndDepsFiles,
    CrateObjectAndDepsFiles,
)> {

    let mut app_objects = CrateObjectFiles::new();
    let mut kernel_files = CrateObjectAndDepsFiles::new();
    let mut other_files = CrateObjectAndDepsFiles::new();

    for dir_entry in fs::read_dir(input_dir)? {
        let dir_entry = dir_entry?;
        let metadata = dir_entry.metadata()?;
        if !metadata.is_file() { continue; }
        let file_name = dir_entry.file_name().into_string().unwrap();
        if !file_name.ends_with(".o") { continue; }
        let file_stem = file_name.split(".o").next().expect("object file name didn't have the .o extension");
        let prefix = file_name.split("-").next().expect("object file name didn't have the crate/hash '-' delimiter");
        let modified_time = metadata.modified()?;

        // A closure for calculating paths for .rmeta and .rlib files in the same directory as the given object file.
        let generate_deps_paths = |obj_file: DirEntry| {
            let mut rmeta_path = obj_file.path();
            rmeta_path.set_file_name(format!("{}{}.{}", DEPS_PREFIX, file_stem, RMETA_EXTENSION));
            let mut rlib_path = rmeta_path.clone();
            rlib_path.set_extension(RLIB_EXTENSION);
            (obj_file, [rmeta_path, rlib_path])
        };

        // Check whether the object file is for a crate designated as an application, kernel, or other crate.
        if app_crates.contains(prefix) {
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
        } else if kernel_crates.contains(prefix) {
            match kernel_files.entry(prefix.to_string()) {
                Entry::Occupied(mut occupied) => {
                    if occupied.get().0.metadata()?.modified()? < modified_time {
                        occupied.insert(generate_deps_paths(dir_entry));
                    }
                }
                Entry::Vacant(vacant) => {
                    vacant.insert(generate_deps_paths(dir_entry));
                }
            }
        } else {
            other_files.insert(file_stem.to_string(), generate_deps_paths(dir_entry));
        }

    }

    // optional debug output
    if PRINT_CRATES {
        println!("APPLICATION OBJECT FILES:");
        print_crates_objects(&app_objects, PRINT_SORTED);
        println!("KERNEL OBJECT FILES AND DEPS FILES:");
        print_crates_objects_and_deps(&kernel_files, PRINT_SORTED);
        println!("OTHER OBJECT FILES AND DEPS FILES:");
        print_crates_objects_and_deps(&other_files, PRINT_SORTED);
    }
    
    Ok((
        app_objects,
        kernel_files,
        other_files,
    ))
}


/// Copies each file in the `files` iterator into the given `output_dir`.
///
/// Prepends the given `prefix` onto the front of the output file names.
/// 
/// Ignores any source files in the `files` iterator that do not exist. 
/// This is a policy choice due to how we form paths for deps files, which may not actually exist. 
fn copy_files<'p, O, P, I>(
    output_dir: O,
    files: I,
    prefix: &str
) -> io::Result<()> 
    where O: AsRef<Path>,
          P: AsRef<Path>,
          I: Iterator<Item = P>,
{
    for source_path_ref in files {
        let source_path = source_path_ref.as_ref();
        let mut dest_path = output_dir.as_ref().to_path_buf();
        dest_path.push(format!("{}{}", prefix, source_path.file_name().and_then(|osstr| osstr.to_str()).unwrap()));

        if PRINT_CRATES {
            println!("Copying {} to {}", source_path.display(), dest_path.display());
        }
            
        match fs::copy(source_path, dest_path) {
            Ok(_bytes_copied) => { }
            Err(e) if e.kind() == io::ErrorKind::NotFound => { }  // Ignore source files that don't exist
            Err(other_err) => return Err(other_err),
        }
    }
    Ok(())
}



fn print_crates_objects(objects: &CrateObjectFiles, sorted: bool) {
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


fn print_crates_objects_and_deps(files: &CrateObjectAndDepsFiles, sorted: bool) {
    if sorted {
        let mut sorted = files.keys().collect::<Vec<&String>>();
        sorted.sort_unstable();
        for o in &sorted {
            println!("\t{}", o);
        }
    } else {
        for (k, v) in files.iter() {
            println!("\t{} --> {}, {}, {}", k, v.0.path().display(), v.1[0].display(), v.1[1].display());
        }
    }
}
