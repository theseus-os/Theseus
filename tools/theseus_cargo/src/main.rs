//! This program is basically a wrapper around cargo that cross-compiles Theseus components
//! in a way that supports out-of-tree builds based on a previous build of Theseus.
//!
//! Specifically, this program can (inefficiently) build a standalone crate in a way that allows
//! that crate to depend upon and link against a set of pre-built crates;
//! those pre-built crates are given as a set of dependencies, primarily `.rmeta` and `.rlib` files.
//! 
//! This program works by invoking Rust's `cargo` build tool and capturing its verbose output
//! such that we can modify and re-run the commands that cargo issued to rustc. 
//!
//! In the future, it may also perform a special form of linking, which I've dubbed "partially-static" linking. 
//!

//! TODO: FIXME: idea: if we want to avoid actually building things twice, we could maybe use a fake no-op wrapper around rustc
//!                    using the `RUSTC` environment variable, which would not actually perform any compilation activity. 

//! TODO: FIXME:  we may want to use the  CARGO_PRIMARY_PACKAGE=1 env variable from -vv build output to identify which is the main package being built. 


// we may wish to use the rustc_metadata crate to parse .rmeta files to get the exact version/hash of each dependency.
// #![feature(rustc_private)] 

extern crate getopts;
extern crate clap;
extern crate walkdir;
extern crate regex;
extern crate itertools;
extern crate toml;
extern crate serde;

use getopts::Options;
use std::{
    collections::HashMap,
    env,
    fs,
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
};
use walkdir::WalkDir;
// use itertools::Itertools;


fn main() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    let _env_vars: HashMap<String, String> = env::vars().collect();

    let mut opts = Options::new();
    opts.parsing_style(getopts::ParsingStyle::StopAtFirstFree);
    opts.reqopt(
        "", 
        "input",  
        "(required) path to the directory of pre-built crates dependency files (.rmeta/.rlib), 
         typically the `target`, e.g., \"/path/to/target/$TARGET/release/deps\"", 
        "INPUT_DIR"
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

    let input_dir_arg  = matches.opt_str("input").expect("required --input arg was not provided");
    let input_dir_path = fs::canonicalize(&input_dir_arg)
        .map_err(|e| format!("--input arg '{}' was invalid path. Error: {}", input_dir_arg, e))?;
    let prebuilt_crates_set = if input_dir_path.is_dir() {
        populate_crates_from_dir(&input_dir_path)
            .map_err(|e| format!("Error parsing --input arg as directory: {}", e))?
    } else {
        return Err(format!("Couldn't access --input argument '{}' as a directory", input_dir_path.display()));
    };

    let build_config = BuildConfig::populate_from_dir(&input_dir_path)?;
    let verbose_count = count_verbose_arg(&matches.free);

    let cargo_cmd_string = matches.free.join(" ");

    let stderr_captured = run_initial_cargo(
        _env_vars,
        cargo_cmd_string.clone(),
        &build_config,
        &input_dir_path,
        verbose_count
    )?;
    // println!("\n\n------------------- STDERR --------------------- \n{}", stderr_captured.join("\n\n"));

    if cargo_cmd_string.split_whitespace().next() != Some("build") {
        println!("Exiting after completing non-'build' cargo command.");
        return Ok(());
    }

    let last_cmd = stderr_captured.last()
        .ok_or_else(|| format!("No commands captured from stderr during the initial cargo command"))?;
    let out_dir = PathBuf::from(get_out_dir_arg(last_cmd)?);

    // Now that we have run the initial cargo build, it has created many redundant dependency artifacts 
    // in the local crate's target/ directory, namely the locally re-built versions of Theseus kernel crates,
    // specifically all the crates that are in the set of prebuilt crates.
    // We need to remove those redundant files from the local target/ directory (the "out-dir")
    // such that when we re-issue the rustc commands below, it won't fail with an error about 
    // multiple "potentially newer" versions of a given crate dependency. 
    for dir_entry in fs::read_dir(&out_dir).unwrap() {
        let dir_entry = dir_entry.unwrap();
        let path = dir_entry.path();
        if !dir_entry.file_type().unwrap().is_file() {
            return Err(format!("Found unexpected non-file entry in out_dir: {}", path.display()));
        }

        // We should remove all potential redundant files, including:
        // * <crate_name>-<hash>.d
        // * <crate_name>-<hash>.o
        // * lib<crate_name>-<hash>.rmeta
        // * lib<crate_name>-<hash>.rlib
        //
        // We do not know the exact hash value appended to each crate, we only know the plain crate name.
        // Here, extract the plain crate_name from the file name.
        let crate_name = match path.extension().and_then(|os_str| os_str.to_str()) {
            Some("d") | Some("o") => {
                get_plain_crate_name_from_path(&path)?
            }
            Some(RMETA_FILE_EXTENSION) | Some(RLIB_FILE_EXTENSION) => {
                let libcrate_name = get_plain_crate_name_from_path(&path)?;
                if libcrate_name.starts_with(RMETA_RLIB_FILE_PREFIX) {
                    &libcrate_name[PREFIX_END ..]
                } else {
                    return Err(format!("Found .rlib or .rmeta file in out_dir that didn't start with 'lib' prefix: {}", path.display()));
                }
            }
            _ => {
                println!("Removing potentially-redundant file with unexpected extension: {}", path.display());
                fs::remove_file(&path).map_err(|e| 
                    format!("Failed to remove potentially-redundant file with unexpected extension: {}. Error: {}", path.display(), e)
                )?;
                continue;
            }
        };

        // See if that crate already exists in our set of prebuilt crates.
        if prebuilt_crates_set.contains_key(crate_name) {
            // remove the redundant file
            println!("### Removing redundant crate file {}", path.display());
            fs::remove_file(&path).map_err(|e| 
                format!("Failed to remove redundant crate file in out_dir: {}. Error: {}", path.display(), e)
            )?;
        } else {
            // Here, do nothing. We must keep the non-redundant files,
            // as they represent new dependencies that were not part of
            // the original in-tree Theseus build.
        }
    }

    // Here, remove the ".fingerprint/` directory, in order to force rustc
    // to rebuild all artifacts for all of the modified rustc commands that we re-run below.
    // Those fingerprint files are in the actual target directory, which is the parent directory of `out_dir`.
    let target_dir = out_dir.parent().unwrap();
    let mut fingerprint_dir_path = target_dir.to_path_buf();
    fingerprint_dir_path.push(".fingerprint");
    println!("--> Removing .fingerprint directory: {}", fingerprint_dir_path.display());
    fs::remove_dir_all(&fingerprint_dir_path).map_err(|_e| 
        format!("Failed to remove .fingerprint directory: {}. Error: {:?}", fingerprint_dir_path.display(), _e)
    )?;

        // if dir_entry.file_type().unwrap().is_file() {
        //     match path.extension().and_then(|os_str| os_str.to_str()) {
        //         Some("d") 
        //         | Some("o")
        //         | Some("a")
        //         | Some(RMETA_FILE_EXTENSION)
        //         | Some(RLIB_FILE_EXTENSION) => {
        //             println!("Removing potentially-redundant target file: {}", path.display());
        //             fs::remove_file(&path).map_err(|e| 
        //                 format!("Failed to remove potentially-redundant target file: {}. Error: {}", path.display(), e)
        //             )?;
        //         }
        //         _ => println!("Skipping existing file with unknown extension in target directory: {:?}", path.display()),
        //     }
        // }


    // Obtain the directory for the host system dependencies
    let mut host_deps_dir_path = PathBuf::from(&input_dir_path);
    host_deps_dir_path.push(&build_config.host_deps);

    // re-execute the rustc commands that we captured from the original cargo verbose output. 
    for original_cmd in &stderr_captured {
        // This function will only re-run rustc for crates that don't already exist in the set of prebuilt crates.
        run_rustc_command(
            original_cmd, 
            &prebuilt_crates_set, 
            &input_dir_path, 
            &input_dir_path,
            &host_deps_dir_path,
        )?;
    }

    // Print out a warning that we don't currently forward cargo features
    // from the original build config for Theseus to the current crate being built.
    if build_config.features.contains("--features") {
        eprintln!("\n\x1b[33;1mImportant warning/note: \x1b[22m
        theseus_cargo currently ignores cargo features used to build Theseus.
        You may need to specify them as features in your this crate's Cargo.toml,
        but this should only be necessary if this crate uses features 
        for Theseus crates that weren't specified in the original build of Theseus. 
        \x1b[1m--> This is currently an untested edge case. <--\x1b[22m
        Features specified and ignored:
          {:?} \x1b[0m",
            build_config.features
        );
    }

    Ok(())
}


/// Parses the given `path` to obtain the part of the filename before the crate name delimiter '-'. 
fn get_plain_crate_name_from_path<'p>(path: &'p Path) -> Result<&'p str, String> {
    path.file_stem()
        .and_then(|os_str| os_str.to_str())
        .ok_or_else(|| format!("Couldn't get file name of file in out_dir: {}", path.display()))?
        .split('-').next()
        .ok_or_else(|| format!("File in out_dir missing delimiter '-' between crate name and hash. {}", path.display()))
}


/// Counts the level of verbosity specified by arguments into `cargo`.
fn count_verbose_arg<'i, S: AsRef<str> + 'i, I: IntoIterator<Item = &'i S>>(args: I) -> usize {
    let mut count = 0;
    for arg in args.into_iter().flat_map(|a| a.as_ref().split_whitespace()) {
        count += match arg.as_ref() {
            "--verbose" | "-v" => 1,
            "-vv" =>  2,
            _ => 0, 
        };
    }
    count
}

fn print_usage(opts: Options) {
    let brief = format!(
        "Usage: theseus_cargo --input INPUT_DIR [OTHER_OPTIONS] CARGO_COMMAND [CARGO OPTIONS]\n\n \
           *** Note: the  --input argument and other options must come before the cargo command. \
           *** Note: generic CARGO_OPTIONS are currently ignored, except for verbose levels."
    ); 
    print!("{}", opts.usage(&brief));
}

// The commands we care about capturing starting with "Running `" and end with "`".
const COMMAND_START:              &str = "Running `";
const COMMAND_END:                &str = "`";
const RUSTC_CMD_START:            &str = "rustc --crate-name";
const BUILD_SCRIPT_CRATE_NAME:    &str = "build_script_build";

// The format of rmeta/rlib file names. 
const RMETA_RLIB_FILE_PREFIX:     &str = "lib";
const RMETA_FILE_EXTENSION:       &str = "rmeta";
const RLIB_FILE_EXTENSION:        &str = "rlib";
const PREFIX_END: usize = RMETA_RLIB_FILE_PREFIX.len();


/// Runs the actual cargo build command.
///
/// Returns the captured content of content written to `stderr` by the cargo command, as a list of lines.
fn run_initial_cargo<P: AsRef<Path>>(
    _env_vars: HashMap<String, String>,
    full_args: String,
    build_config: &BuildConfig,
    input_dir_path: P,
    verbose_level: usize
) -> Result<Vec<String>, String> {
    let input_dir_path = input_dir_path.as_ref();
    // println!("FULL ARGS: {}", full_args);

    let subcommand = full_args.split_whitespace()
        .next()
        .ok_or_else(|| format!("Missing subcommand argument to `theseus_cargo` (e.g., `build`)"))?;
    
    if subcommand != "build" {
        return Err(
            format!("cargo commands other than `build` are not supported. You tried to run subcommand {:?}.", subcommand)
        );
    }

    let mut cmd = Command::new("cargo");
    cmd.arg(subcommand)
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());
    
    // Ensure that we use only the arguments specifed by the Theseus build config.
    cmd.args(build_config.cargoflags.split_whitespace())
        .arg("--target").arg(&build_config.target);

    // Ensure that we run the cargo command with the maximum verbosity level, which is -vv.
    cmd.arg("-vv");

    // Use full color output to get a regular terminal-esque display from cargo. 
    cmd.arg("--color=always");

    // Add the requisite environment variables to configure cargo such that rustc builds with the proper config
    // and it can locate our special target json file. 
    cmd.env("RUST_TARGET_PATH", input_dir_path);
    // Add the sysroot argument to our rustflags so cargo will use our pre-built cross-compiled (for Theseus) core dependencies. 
    let mut sysroot_path = PathBuf::from(input_dir_path);
    sysroot_path.push(&build_config.sysroot);
    let rustflags = format!("{} --sysroot {}", build_config.rustflags, sysroot_path.display());
    cmd.env("RUSTFLAGS", rustflags);


    println!("\nRunning initial cargo command:\n{:?}", cmd);
    cmd.get_envs().for_each(|(k, v)|println!("\t### env {:?} = {:?}", k, v));

    // Run the actual cargo command.
    let mut child_process = cmd.spawn()
        .map_err(|io_err| format!("Failed to run cargo command: {:?}", io_err))?;
    
    // We read the stderr output in this thread and create a new thread to read the stdout output.
    let stdout = child_process.stdout.take().ok_or_else(|| format!("Could not capture stdout."))?;
    let t = thread::spawn(move || {
        let stdout_reader = BufReader::new(stdout);
        let mut stdout_logs: Vec<String> = Vec::new();
        stdout_reader.lines()
            .filter_map(|line| line.ok())
            .for_each(|line| {
                // Cargo only prints to stdout for build script output, only if very verbose.
                if verbose_level >= 2 {
                    println!("{}", line);
                }
                stdout_logs.push(line);
            });
        stdout_logs
    });

    let stderr = child_process.stderr.take().ok_or_else(|| format!("Could not capture stderr."))?;
    let stderr_reader = BufReader::new(stderr);
    let mut stderr_logs: Vec<String> = Vec::new();

    // Use regex to strip out the ANSI color codes emitted by the cargo command
    let ansi_escape_regex = regex::Regex::new(r"[\x1B\x9B]\[[^m]+m").unwrap();
    
    let mut pending_multiline_cmd = false;
    let mut original_multiline = String::new();

    // Capture every line that cargo writes to stderr. 
    // We only re-echo the lines that should be outputted by the verbose level specified.
    // The complexity below is due to the fact that a verbose command printed by cargo
    // may span multiple lines, so we need to detect the beginning and end of a multi-line command
    // and merge it into a single line in our captured output. 
    stderr_reader.lines()
        .filter_map(|line| line.ok())
        .for_each(|original_line| {
            let replaced = ansi_escape_regex.replace_all(&original_line, "");
            let line_stripped = replaced.trim_start();

            let is_final_line = 
                (line_stripped.contains("--crate-name") && line_stripped.contains("--crate-type"))
                || line_stripped.ends_with("build-script-build`");

            if line_stripped.starts_with(COMMAND_START) {
                // Here, we've reached the beginning of a rustc command, which we actually do care about. 
                stderr_logs.push(line_stripped.to_string());
                pending_multiline_cmd = !is_final_line;
                original_multiline = String::from(&original_line);
                if !is_final_line {
                    return; // continue to the next line
                }
            } else {
                // Here, we've reached another line, which *may* be the continuation of a previous rustc command,
                // or it may just be a completely irrelevant line of output.
                if pending_multiline_cmd {
                    // append to the latest line of output instead of adding a new line
                    let last = stderr_logs.last_mut().expect("BUG: stderr_logs had no last element");
                    last.push(' ');
                    last.push_str(line_stripped);
                    original_multiline.push('\n');
                    original_multiline.push_str(&original_line);
                    pending_multiline_cmd = !is_final_line;
                    if !is_final_line {
                        return; // continue to the next line
                    }
                } else {
                    // Here: this is an unrelated line of output that isn't a command we want to capture.
                    original_multiline.clear(); // = String::from(&original_line);
                }
            }

            // In the above cargo command, we added a verbose argument to capture the commands issued from cargo to rustc. 
            // But if the user didn't ask for that, then we shouldn't print that verbose output here. 
            // Verbose output lines start with "Running `", "+ ", or "[".
            let should_print = |stripped_line: &str| {
                verbose_level > 0 ||  // print everything if verbose
                (
                    // print only "Compiling" and warning/error lines if not verbose
                    !stripped_line.starts_with("+ ")
                    && !stripped_line.starts_with("[")
                    && !stripped_line.starts_with(COMMAND_START)
                )
            };
            if !original_multiline.is_empty() && is_final_line {
                let original_multiline_replaced = ansi_escape_regex.replace_all(&original_multiline, "");
                let original_multiline_stripped = original_multiline_replaced.trim_start();
                if should_print(original_multiline_stripped) {
                    eprintln!("{}", original_multiline)
                }
            } else if should_print(line_stripped) {
                eprintln!("{}", original_line);
            }
        });
    
    let _stdout_logs = t.join().unwrap();

    let exit_status = child_process.wait()
        .map_err(|io_err| format!("Failed to wait for cargo process to finish. Error: {:?}", io_err))?;
    match exit_status.code() {
        Some(0) => { }
        Some(code) => return Err(format!("cargo command completed with failed exit code {}", code)),
        _ => return Err(format!("cargo command was killed")),
    }

    Ok(stderr_logs)
}


/// Returns true if the given `arg` should be ignored in our rustc invocation. 
fn ignore_arg(arg: &str) -> bool {
    arg == "--error-format" ||
    arg == "--json"
}


/// Takes the given `original_cmd` that was captured from the verbose output of cargo,
/// and parses/modifies it to link against (depend on) the corresponding crate of the same name
/// from the list of prebuilt crates. 
///
/// The actual dependency files (.rmeta/.rlib) for the prebuilt crates should be located in the `prebuilt_dir`. 
/// The target specification JSON file should be found in the `target_dir_path`. 
/// These two directories are usually the same directory.
///
/// # Return
/// * Returns `Ok(true` if everything works and the modified rustc command executes properly. 
/// * Returns `Ok(false)` if no action needs to be taken. 
///   This occurs if `original_cmd` is for building a build script (currently ignored), 
///   or if `original_cmd` is for building a crate that already exists in the set of `prebuilt_crates`.
/// * Returns an error if the command fails. 
fn run_rustc_command<P: AsRef<Path>, T: AsRef<Path>, H: AsRef<Path>>(
    original_cmd: &str,
    prebuilt_crates: &HashMap<String, String>,
    prebuilt_dir: P,
    target_dir_path: T,
    host_deps_dir_path: H,
) -> Result<bool, String> {
    let prebuilt_dir = prebuilt_dir.as_ref();
    let target_dir_path = target_dir_path.as_ref();
    let host_deps_dir_path = host_deps_dir_path.as_ref();

    let command = if original_cmd.starts_with(COMMAND_START) && original_cmd.ends_with(COMMAND_END) {
        let end_index = original_cmd.len() - COMMAND_END.len();
        &original_cmd[COMMAND_START.len() .. end_index]
    } else {
        return Err(format!("Unexpected formatting in capture command (must start with {:?} and end with {:?}. Command: {:?}", 
            original_cmd, COMMAND_START, COMMAND_END,
        ));
    };

    // Skip invocations of build scripts, as I don't think we need to re-run those. 
    // If this turns out to be wrong and we do need to run them, we need to change this logic to simply re-run it
    // and skip pretty much the rest of this entire function.
    if command.ends_with("build-script-build") {
        return Ok(false);
    }
    
    println!("\n\nLooking at original command:\n{}", command);
    let start_of_rustc_cmd = command.find(RUSTC_CMD_START).ok_or_else(|| 
        format!("Couldn't find {:?} in command:\n{:?}", RUSTC_CMD_START, command)
    )?;
    let _rustc_env_vars     = &command[.. start_of_rustc_cmd];
    let command_without_env = &command[start_of_rustc_cmd ..];

    // The arguments in the command that we care about are:
    //  *  "-L dependency=<dir>"
    //  *  "--extern <crate_name>=<crate_file>.rmeta"
    //
    // Below, we use `clap` to find those argumnets and replace them. 
    //
    // First, we parse the following part:
    // "rustc --crate-name <crate_name> <crate_source_file> <all_other_args>"
    let top_level_matches = rustc_clap_options("rustc")
        .setting(clap::AppSettings::DisableHelpFlags)
        .setting(clap::AppSettings::DisableHelpSubcommand)
        .setting(clap::AppSettings::AllowExternalSubcommands)
        .setting(clap::AppSettings::ColorNever)
        .get_matches_from_safe(command_without_env.split_whitespace());
    
    let top_level_matches = top_level_matches.map_err(|e| 
        format!("Missing support for argument found in captured rustc command: {}", e)
    )?;

    // Clap will parse the args as such:
    // * the --crate-name will be the first argument
    // * the path to the crate's main file will be the first subcommand
    // * that subcommand's arguments will include ALL OTHER arguments that we care about, specified below.

    let crate_name = top_level_matches.value_of("--crate-name").expect("rustc command did not have required --crate-name argument");
    let (crate_source_file, additional_args) = top_level_matches.subcommand();
    let additional_args = additional_args.unwrap();

    // Skip build script invocations, as I don't think we need to re-run those. 
    if crate_name == BUILD_SCRIPT_CRATE_NAME {
        println!("\n### Skipping build script build");
        return Ok(false);
    }

    // Skip crates that have already been built. (Not sure if this is always 100% correct)
    if prebuilt_crates.contains_key(crate_name) {
        println!("\n### Skipping already-built crate {:?}", crate_name);
        return Ok(false);
    }

    let args_after_source_file = additional_args.values_of("").unwrap();

    // Second, we parse all other args in the command that followed the crate source file. 
    // Note that the arg name, the parameter in with_name(), in each arg below MUST BE exactly how it is invoked by cargo.
    let matches = rustc_clap_options("")
        .setting(clap::AppSettings::DisableHelpFlags)
        .setting(clap::AppSettings::DisableHelpSubcommand)
        .setting(clap::AppSettings::ColorNever)
        .setting(clap::AppSettings::NoBinaryName)
        .get_matches_from_safe(args_after_source_file);
    
    let matches = matches.map_err(|e| 
        format!("Missing support for argument found in captured rustc command: {}", e)
    )?;


    // Now, re-create the rustc command invocation with the proper arguments.
    // First, we handle the --crate-name and --edition arguments, which may come before the crate source file path. 
    let mut recreated_cmd = Command::new("rustc");
    recreated_cmd.arg("--crate-name").arg(crate_name);
    if let Some(edition) = top_level_matches.value_of("--edition") {
        recreated_cmd.arg("--edition").arg(edition);
    }
    recreated_cmd.arg(crate_source_file);

    let mut args_changed = false;

    // After adding the initial stuff: rustc command, crate name, (optional --edition), and crate source file, 
    // the other arguments are added in the loop below. 
    'args_label: 
    for (&arg, values) in matches.args.iter() {
        println!("Arg {:?} has values:\n\t {:?}", arg, values.vals);
        if ignore_arg(arg) { continue; }

        for value in &values.vals {
            let value = value.to_string_lossy();
            let mut new_value = value.to_string();

            if arg == "--crate-type" && value == "proc-macro" {
                // Don't re-run proc_macro builds, as those are built to run on the host.
                args_changed = false;
                break 'args_label;
            } else if arg == "--extern" {
                let rmeta_or_rlib_extension = if value.ends_with(RMETA_FILE_EXTENSION) {
                    Some(RMETA_FILE_EXTENSION)
                } else if value.ends_with(RLIB_FILE_EXTENSION) {
                    Some(RLIB_FILE_EXTENSION)
                } else if value == "proc_macro" {
                    None
                } else {
                    // println!("Skipping non-rlib --extern value: {:?}", value);
                    return Err(format!("Unsupported --extern arg value {:?}. We only support '.rlib' or '.rmeta' files.", value));
                };

                if let Some(extension) = rmeta_or_rlib_extension {
                    let (extern_crate_name, crate_rmeta_path) = value
                        .find('=')
                        .map(|idx| value.split_at(idx))
                        .map(|(name, path)| (name, &path[1..])) // ignore the '=' delimiter
                        .ok_or_else(|| format!("Failed to parse value of --extern arg as CRATENAME=PATH: {:?}", value))?;
                    println!("Found --extern arg, {:?} --> {:?}", extern_crate_name, crate_rmeta_path);
                    if let Some(extern_crate_name_with_hash) = prebuilt_crates.get(extern_crate_name) {
                        let mut new_crate_path = prebuilt_dir.to_path_buf();
                        new_crate_path.push(format!("{}{}.{}", RMETA_RLIB_FILE_PREFIX, extern_crate_name_with_hash, extension));
                        println!("#### Replacing crate {:?} with prebuilt crate at {}", extern_crate_name, new_crate_path.display());
                        new_value = format!("{}={}", extern_crate_name, new_crate_path.display());
                    }
                }
            } else if arg == "-L" {
                let (kind, _path) = value.as_ref()
                    .find('=')
                    .map(|idx| value.split_at(idx))
                    .map(|(kind, path)| (kind, &path[1..])) // ignore the '=' delimiter
                    .ok_or_else(|| format!("Failed to parse value of -L arg as KIND=PATH: {:?}", value))?;
                // println!("Found -L arg, {:?} --> {:?}", kind, _path);
                if !(kind == "dependency" || kind == "native") {
                    println!("WARNING: Unsupported -L arg value {:?}. We only support 'dependency=PATH' or 'native=PATH'.", value);
                }
                // TODO: if we need to actually modify any -L argument values, then set `new_value` accordingly here. 
            }

            if value != new_value {
                args_changed = true;
            } 
            recreated_cmd.arg(arg);
            recreated_cmd.arg(new_value);
        }
    }

    // If any args actually changed, we need to run the re-created command. 
    if args_changed {
        // Add our directory of prebuilt crates as a library search path, for dependency resolution. 
        // This is okay because we removed all of the potentially conflicting crates from the local target/ directory,
        // which ensures that adding in the directory of prebuilt crate .rmeta/.rlib files won't cause rustc to complain
        // about multiple "potentially newer" versions of a given crate.
        recreated_cmd.arg("-L").arg(prebuilt_dir);
        // We also need to add the directory of host dependencies, e.g., proc macro crates and such. 
        recreated_cmd.arg("-L").arg(host_deps_dir_path);

        // println!("\n\n--------------- Inherited Environment Variables ----------------\n");
        // let _env_cmd = Command::new("env").spawn().unwrap().wait().unwrap();
        println!("About to execute recreated_cmd that had changed arguments:\n{:?}", recreated_cmd);
    } else {
        println!("### Args did not change, skipping recreated_cmd:\n{:?}", recreated_cmd);
        return Ok(false);
    }

    // println!("Press enter to run the above command ...");
    // let mut buf = String::new();
    // io::stdin().read_line(&mut buf).expect("failed to read stdin");

    // Ensure we have the RUST_TARGET_PATH env var so that rustc can find our target spec JSON file. 
    recreated_cmd.env("RUST_TARGET_PATH", target_dir_path);

    // As far as I can tell, we do not need to include or modify the environment variable `LD_LIBRARY_PATH`. 
    // It seems like rustc doesn't need it, but if we decide it does, add that in here.

    // Finally, we run the recreated rustc command.
    let mut rustc_process = recreated_cmd.spawn().map_err(|io_err| 
        format!("Failed to run cargo command: {:?}", io_err)
    )?;
    let exit_status = rustc_process.wait().map_err(|io_err| 
        format!("Error running rustc: {}", io_err)
    )?;

    match exit_status.code() {
        Some(0) => {
            println!("Ran rustc command (modified for Theseus) successfully.");
            Ok(true)
        }
        Some(code) => Err(format!("rustc command exited with failure code {}", code)),
        _ => Err(format!("rustc command failed and was killed.")),
    }
}


/// Parse the given verbose rustc command string and return the value of the "--out-dir" argument.
fn get_out_dir_arg(cmd_str: &str) -> Result<String, String> {
    let out_dir_str_start = cmd_str.find(" --out-dir")
        .map(|idx| &cmd_str[idx..])
        .ok_or_else(|| format!("Captured rustc command did not have an --out-dir argument"))?;
    let out_dir_parse = rustc_clap_options("")
        .setting(clap::AppSettings::DisableHelpFlags)
        .setting(clap::AppSettings::DisableHelpSubcommand)
        .setting(clap::AppSettings::AllowExternalSubcommands)
        .setting(clap::AppSettings::NoBinaryName)
        .setting(clap::AppSettings::ColorNever)
        .get_matches_from_safe(out_dir_str_start.split_whitespace());
    let matches = out_dir_parse.map_err(|e| 
        format!("Could not parse --out-dir argument in captured rustc command.\nError {}", e)
    )?;
    matches.value_of("--out-dir")
        .map(String::from)
        .ok_or_else(|| format!("--out-dir argument did not have a value"))
}


/// Iterates over the contents of the given directory to find crates within it. 
/// 
/// This directory should contain one .rmeta and .rlib file per crate, 
/// and those files are named as such:
/// `"lib<crate_name>-<hash>.[rmeta]"`
/// 
/// This function only looks at the `.rmeta` files in the given directory 
/// and extracts from that file name the name of the crate name as a String.
/// 
/// Returns the set of discovered crates as a map, in which the key is the simple crate name 
/// ("my_crate") and the value is the full crate name with the hash included ("my_crate-43462c60d48a531a").
/// The value can be used to define the path to crate's actual .rmeta/.rlib file.
fn populate_crates_from_dir<P: AsRef<Path>>(dir_path: P) -> Result<HashMap<String, String>, io::Error> {
    let mut crates: HashMap<String, String> = HashMap::new();
    
    let dir_iter = WalkDir::new(dir_path)
        .into_iter()
        .filter_map(|res| res.ok());

    for dir_entry in dir_iter {
        if !dir_entry.file_type().is_file() { continue; }
        let path = dir_entry.path();
        if path.extension().and_then(|p| p.to_str()) == Some(RMETA_FILE_EXTENSION) {
            let filestem = path.file_stem().expect("no valid file stem").to_string_lossy();
            if filestem.starts_with("lib") {
                let crate_name_with_hash = &filestem[PREFIX_END ..];
                let crate_name_without_hash = crate_name_with_hash.split('-').next().unwrap();
                crates.insert(crate_name_without_hash.to_string(), crate_name_with_hash.to_string());
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("File {:?} is an .rmeta file that does not begin with 'lib' as expected.", path),
                ));
            }
        }
    }
    Ok(crates)
}


use serde::Deserialize;

/// Config and environment variables that describe a prior build of Theseus. 
/// These are parsed from the `TheseusBuild.toml` file created in the top-level Makefile.
#[derive(Deserialize, Debug)]
struct BuildConfig {
    target:        String,
    sysroot:       PathBuf,
    rustflags:     String,
    cargoflags:    String,
    features:      String,
    host_deps:     PathBuf,
}
impl BuildConfig {
    fn populate_from_dir<P: Into<PathBuf>>(dir_path: P) -> Result<BuildConfig, String> {
        let mut pathbuf = dir_path.into();
        pathbuf.push("TheseusBuild.toml");
        let theseus_build_toml = fs::read_to_string(&pathbuf)
            .map_err(|e| format!("Error reading {}: {}", pathbuf.display(), e))?;
        let mut build_config: BuildConfig = toml::from_str(&theseus_build_toml)
            .map_err(|e| format!("Error parsing TheseusBuild.toml: {}", e))?;
        
        // Trim errant whitespace from the various parsed items.
        build_config.target     = build_config.target    .trim().into();
        build_config.rustflags  = build_config.rustflags .trim().into();
        build_config.cargoflags = build_config.cargoflags.trim().into();
        build_config.features   = build_config.features  .trim().into();

        Ok(build_config)
    }
}



/// Creates a `Clap::App` instance that handles all (most) of the command-line arguments
/// accepted by the `rustc` executable. 
///
/// I obtained this by looking at the output of `rustc --help --verbose`. 
fn rustc_clap_options<'a, 'b>(app_name: &str) -> clap::App<'a, 'b> {
    clap::App::new(app_name)
        // The first argument that we want to see, --crate-name.
        .arg(clap::Arg::with_name("--crate-name")
            .long("crate-name")
            .takes_value(true)
        )

        // Note: add any other arguments that you encounter in a rustc invocation here.

        .arg(clap::Arg::with_name("-L")
            .short("L")
            .takes_value(true)
            .multiple(true)
        )
        .arg(clap::Arg::with_name("-l")
            .short("l")
            .takes_value(true)
            .multiple(true)
        )
        .arg(clap::Arg::with_name("--extern")
            .long("extern")
            .takes_value(true)
            .multiple(true)
        )
        .arg(clap::Arg::with_name("-C")
            .short("C")
            .long("codegen")
            .takes_value(true)
            .multiple(true)
        )
        .arg(clap::Arg::with_name("-W")
            .short("W")
            .long("warn")
            .takes_value(true)
            .multiple(true)
        )
        .arg(clap::Arg::with_name("-A")
            .short("A")
            .long("allow")
            .takes_value(true)
            .multiple(true)
        )
        .arg(clap::Arg::with_name("-D")
            .short("D")
            .long("deny")
            .takes_value(true)
            .multiple(true)
        )
        .arg(clap::Arg::with_name("-F")
            .short("F")
            .long("forbid")
            .takes_value(true)
            .multiple(true)
        )
        .arg(clap::Arg::with_name("--cap-lints")
            .long("cap-lints")
            .takes_value(true)
            .multiple(true)
        )
        .arg(clap::Arg::with_name("-Z")
            .short("Z")
            .takes_value(true)
            .multiple(true)
        )
        .arg(clap::Arg::with_name("--crate-type")
            .long("crate-type")
            .takes_value(true)
            .multiple(true)
        )
        .arg(clap::Arg::with_name("--emit")
            .long("emit")
            .takes_value(true)
            .multiple(true)
        )
        .arg(clap::Arg::with_name("--edition")
            .long("edition")
            .takes_value(true)
        )
        .arg(clap::Arg::with_name("-g")
            .short("g")
        )
        .arg(clap::Arg::with_name("-O")
            .short("O")
        )
        .arg(clap::Arg::with_name("--out-dir")
            .long("out-dir")
            .takes_value(true)
        )
        .arg(clap::Arg::with_name("--error-format")
            .long("error-format")
            .takes_value(true)
        )
        .arg(clap::Arg::with_name("--json")
            .long("json")
            .takes_value(true)
        )
        .arg(clap::Arg::with_name("--target")
            .long("target")
            .takes_value(true)
        )
        .arg(clap::Arg::with_name("--sysroot")
            .long("sysroot")
            .takes_value(true)
        )
        .arg(clap::Arg::with_name("--edition")
            .long("edition")
            .takes_value(true)
        )
        .arg(clap::Arg::with_name("--cfg")
            .long("cfg")
            .takes_value(true)
            .multiple(true)
        )
        .arg(clap::Arg::with_name("--verbose")
            .short("v")
            .long("verbose")
            .takes_value(false)
            .multiple(true)
        )
        .arg(clap::Arg::with_name("--remap-path-prefix")
            .long("remap-path-prefix")
            .takes_value(false)
            .multiple(true)
        )
}
