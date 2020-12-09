//! This program is basically a wrapper around cargo that cross-compiles Theseus components
//! in a way that supports out-of-tree builds based on a set of pre-built Theseus crates. 
//!
//! Specifically, this program can (inefficiently) build a standalone crate in a way that allows
//! that crate to depend upon and link against a set of pre-built crates. 
//! This requires a set of prebuilt dependencies, specified as the `.rmeta` and `.rlib` files.
//! 
//! This program works by invoking the `xargo` program (which itself is a wrapper around Rust's `cargo`) 
//! and watching its output 
//!
//! It also performs a special form of linking, which I've dubbed "partially-static" linking. 
//!
//! TODO: FIXME: finish this documentation once the tool is complete. 
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

use getopts::Options;
use std::{
    collections::{
        HashSet,
        HashMap,
    },
    env,
    fs,
    io::{self, BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
    thread,
};
use walkdir::WalkDir;
// use itertools::Itertools;


fn main() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    let env_vars: HashMap<String, String> = env::vars().collect();
    // println!("----------- Command-line Arguments ----------\n{:#?}", args);
    // println!("----------- Environment Variables -----------\n{:#?}", env_vars);

    let mut opts = Options::new();
    opts.parsing_style(getopts::ParsingStyle::StopAtFirstFree);
    opts.reqopt(
        "", 
        "input",  
        "(required) path to the directory of pre-built crates dependency files (.rmeta/.rlib), 
         typically the `target`, e.g., \"/path/to/target/$TARGET/release/deps\"", 
        "INPUT_DIR"
    );
    // opts.reqopt(
    //     "k", 
    //     "kernel",  
    //     "(required) path to either the directory of kernel crates,
    //      or a file listing each kernel crate name, one per line",
    //     "KERNEL_CRATES"
    // );
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
        .map_err(|e| format!("--input arg {:?} was invalid path, error: {:?}", input_dir_arg, e))?;
    let prebuilt_crates_set = if input_dir_path.is_dir() {
        populate_crates_from_dir(&input_dir_path)
            .map_err(|e| format!("Error parsing --input arg as directory: {:?}", e))?
    } else {
        return Err(format!("Couldn't access --input argument {:?} as a directory", input_dir_path));
    };

    let cargo_cmd_string = matches.free.join(" ");

    let verbose_count = count_verbose_arg(&matches.free);
    println!("VERBOSE_LEVEL: {:?}", verbose_count);

    let stderr_captured = run_initial_xargo(env_vars, cargo_cmd_string.clone(), verbose_count)?;
    // println!("\n\n------------------- STDERR --------------------- \n{}", stderr_captured.join("\n\n"));

    if cargo_cmd_string.split_whitespace().next() != Some("build") {
        println!("Exiting after completing non-'build' xargo command.");
        return Ok(());
    }


    // re-execute the rustc commands that we captured from the original xargo/cargo verbose output. 
    for original_cmd in &stderr_captured {
        run_rustc_command(original_cmd, &prebuilt_crates_set, input_dir_path.as_path())?;
    }


    Ok(())
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
    let brief = format!("Usage: theseus_xargo --input INPUT_DIR [OPTIONS] CARGO_COMMAND [CARGO OPTIONS]");
    print!("{}", opts.usage(&brief));
}

// The commands we care about capturing starting with "Running `" and end with "`".
const COMMAND_START: &str = "Running `";
const COMMAND_END:   &str = "`";
const RUSTC_START:   &str = "rustc --crate-name";
const BUILD_SCRIPT:  &str = "build-script-build";


/// Runs the actual xargo command, e.g., xargo build, 
/// with all of the arguments specified on the command line. 
///
/// Returns the captured content of content written to `stderr` by the xargo command,
/// as a list of lines.
fn run_initial_xargo(_env_vars: HashMap<String, String>, full_args: String, verbose_level: usize) -> Result<Vec<String>, String> {
    println!("FULL ARGS: {}", full_args);

    let mut cmd = Command::new("xargo");
    cmd.args(full_args.split_whitespace())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());
    
    // Ensure that we run the xargo command with the maximum verbosity level, which is -vv.
    cmd.arg("-vv");

    // Run the actual xargo command.
    let mut child_process = cmd.spawn()
        .map_err(|io_err| format!("Failed to run xargo command: {:?}", io_err))?;
    
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

            // In the above xargo command, we added a verbose argument to capture the commands issued from xargo/cargo to rustc. 
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
        .map_err(|io_err| format!("Failed to wait for xargo process to finish. Error: {:?}", io_err))?;
    match exit_status.code() {
        Some(0) => { }
        Some(code) => return Err(format!("xargo command completed with failed exit code {}", code)),
        _ => return Err(format!("xargo command was killed")),
    }

    Ok(stderr_logs)
}


/// Takes the given `original_cmd` that was captured from the verbose output of cargo/xargo,
/// and parses/modifies it to link against (depend on) the corresponding crate of the same name
/// from the list of prebuilt crates. 
///
/// The actual dependency files (.rmeta/.rlib) for the prebuilt crates should be located in the `prebuilt_dir`. 
///
/// Returns an error if the command fails. 
fn run_rustc_command(
    original_cmd: &str,
    prebuilt_crates: &HashSet<String>,
    prebuilt_dir: &Path
) -> Result<(), String> {
    let command = if original_cmd.starts_with(COMMAND_START) && original_cmd.ends_with(COMMAND_END) {
        let end_index = original_cmd.len() - COMMAND_END.len();
        &original_cmd[COMMAND_START.len() .. end_index]
    } else {
        return Err(format!("Unexpected formatting in capture command (must start with {:?} and end with {:?}. Command: {:?}", 
            original_cmd, COMMAND_START, COMMAND_END,
        ));
    };

    // Skip build script invocations, as I don't think we need to re-run those. 
    if command.ends_with(BUILD_SCRIPT) {
        return Ok(());
    }
    
    println!("\n\nLooking at original command:\n{}", command);
    let start_of_rustc_cmd = command.find(RUSTC_START).ok_or_else(|| 
        format!("Couldn't find {:?} in command:\n{:?}", RUSTC_START, command)
    )?;
    let command_without_env = &command[start_of_rustc_cmd ..];

    // The arguments in the command that we care about are:
    //  *  "-L dependency=<dir>"
    //  *  "--extern <crate_name>=<crate_file>.rmeta"
    //
    // Below, we use `clap` to find those argumnets and replace them. 
    //
    // First, we parse the following part:
    // "rustc --crate-name <crate_name> <crate_source_file> <all_other_args>"
    let top_level_matches = clap::App::new("rustc")
        // The first argument that we want to see, --crate-name.
        .arg(clap::Arg::with_name("crate-name")
            .long("crate-name")
            .takes_value(true)
            .required(true)
        )
        .setting(clap::AppSettings::DisableHelpFlags)
        .setting(clap::AppSettings::DisableHelpFlags)
        .setting(clap::AppSettings::AllowExternalSubcommands)
        .setting(clap::AppSettings::ColorNever)
        .get_matches_from_safe(command_without_env.split_whitespace());
    
    let top_level_matches = top_level_matches.map_err(|e| 
        format!("Missing support for argument found in captured rustc command: {}", e)
    )?;
    // println!("\nTop-level Matches: {:#?}", top_level_matches);

    // Clap will parse the args as such:
    // * the --crate-name will be the first argument
    // * the path to the crate's main file will be the first subcommand
    // * that subcommand's arguments will include ALL OTHER arguments that we care about, specified below.

    let crate_name = top_level_matches.value_of("crate-name").unwrap();
    let (crate_source_file, additional_args) = top_level_matches.subcommand();
    let additional_args = additional_args.unwrap();

    println!("\nGot match info:\ncrate-name: {:?}\ncrate_source_file: {:?}\nadditional_args: {:#?}", crate_name, crate_source_file, additional_args);
    let args_after_source_file = additional_args.values_of("").unwrap();

    // Second, we parse all other args in the command that followed the crate source file. 
    let matches = clap::App::new("")
        // The -L linker arg we care about
        .arg(clap::Arg::with_name("L")
            .short("L")
            .takes_value(true)
            .multiple(true)
        )
        // The --extern crate arg we care about
        .arg(clap::Arg::with_name("extern")
            .long("extern")
            .takes_value(true)
            .multiple(true)
        )
        // We don't care about -C arguments
        .arg(clap::Arg::with_name("C")
            .short("C")
            .takes_value(true)
            .multiple(true)
        )
        // We don't care about -D arguments
        .arg(clap::Arg::with_name("DZ")
            .short("D")
            .takes_value(true)
            .multiple(true)
        )
        // We don't care about -Z arguments
        .arg(clap::Arg::with_name("")
            .short("Z")
            .takes_value(true)
            .multiple(true)
        )
        // We don't care about the --crate-type argument
        .arg(clap::Arg::with_name("crate-type")
            .long("crate-type")
            .takes_value(true)
        )
        // We don't care about the --emit arguments
        .arg(clap::Arg::with_name("emit")
            .long("emit")
            .takes_value(true)
            .multiple(true)
        )
        // We don't care about the --out-dir argument
        .arg(clap::Arg::with_name("out-dir")
            .long("out-dir")
            .takes_value(true)
        )
        // We don't care about the --error-format argument
        .arg(clap::Arg::with_name("error-format")
            .long("error-format")
            .takes_value(true)
        )
        // We don't care about the --json argument
        .arg(clap::Arg::with_name("json")
            .long("json")
            .takes_value(true)
        )
        // We don't care about the --target argument
        .arg(clap::Arg::with_name("target")
            .long("target")
            .takes_value(true)
        )
        // We don't care about the --sysroot argument
        .arg(clap::Arg::with_name("sysroot")
            .long("sysroot")
            .takes_value(true)
        )
        // We don't care about the --cfg arguments
        .arg(clap::Arg::with_name("cfg")
            .long("cfg")
            .takes_value(true)
            .multiple(true)
        )
        // We don't care about the --cap-lints arguments
        .arg(clap::Arg::with_name("cap-lints")
            .long("cap-lints")
            .takes_value(true)
            .multiple(true)
        )
        // Note: add any other arguments that you encounter in a rustc invocation here.
        .setting(clap::AppSettings::DisableHelpFlags)
        .setting(clap::AppSettings::DisableHelpFlags)
        .setting(clap::AppSettings::AllowExternalSubcommands)
        .setting(clap::AppSettings::ColorNever)
        .get_matches_from_safe(args_after_source_file);
    
    let matches = matches.map_err(|e| 
        format!("Missing support for argument found in captured rustc command: {}", e)
    )?;

    println!("\n\nMatches: {:#?}", matches);



    let env = &command[.. start_of_rustc_cmd];

    Ok(())
}



/// Iterates over the contents of the given directory to find crates within it. 
/// 
/// This directory should contain one .rmeta and .rlib file per crate, 
/// and those files are named as such:
/// `"lib<crate_name>-<hash>.[rmeta]"`
/// 
/// This function only looks at the `.rmeta` files in the given directory 
/// and extracts from that file name the name of the crate name as a String.
/// That String consists of `"<crate_name>-<hash>", and the set of unique crate names is returned.
fn populate_crates_from_dir<P: AsRef<Path>>(dir_path: P) -> Result<HashSet<String>, io::Error> {
    const RMETA_FILE_EXTENSION: &str = "rmeta";
    const RMETA_FILE_PREFIX:    &str = "lib";
    const PREFIX_END: usize = RMETA_FILE_PREFIX.len();

    let mut crates: HashSet<String> = HashSet::new();
    
    let dir_iter = WalkDir::new(dir_path)
        .into_iter()
        .filter_map(|res| res.ok());

    for dir_entry in dir_iter {
        if !dir_entry.file_type().is_file() { continue; }
        let path = dir_entry.path();
        if path.extension().and_then(|p| p.to_str()) == Some(RMETA_FILE_EXTENSION) {
            let filestem = path.file_stem().expect("no valid file stem").to_string_lossy();
            if filestem.starts_with("lib") {
                crates.insert(filestem[PREFIX_END ..].to_string());
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

