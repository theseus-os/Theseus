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
    println!("----------- Command-line Arguments ----------\n{:#?}", args);
    println!("----------- Environment Variables -----------\n{:#?}", env_vars);

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
    println!("\n\n------------------- STDERR --------------------- \n{}", stderr_captured.join("\n\n"));

    if cargo_cmd_string.split_whitespace().next() != Some("build") {
        println!("Exiting after completing non-'build' xargo command.");
        return Ok(());
    }

    let mut prebuilts_sorted: Vec<String> = prebuilt_crates_set.into_iter().collect();
    prebuilts_sorted.sort();
    println!("\n\n------------------- PREBUILT CRATES --------------------- \n{}", prebuilts_sorted.join("\n"));
    

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

/// Runs the actual xargo command, e.g., xargo build, 
/// with all of the arguments specified on the command line. 
///
/// Returns the captured content of content written to `stderr` by the xargo command,
/// as a list of lines.
fn run_initial_xargo(_env_vars: HashMap<String, String>, full_args: String, verbose_count: usize) -> Result<Vec<String>, String> {
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
                if verbose_count >= 2 {
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

    stderr_reader.lines()
        .filter_map(|line| line.ok())
        .for_each(|original_line| {
            let replaced = ansi_escape_regex.replace_all(&original_line, "");
            let line_stripped = replaced.trim_start();

            let is_final_line = 
                (line_stripped.contains("--crate-name") && line_stripped.contains("--crate-type"))
                || line_stripped.ends_with("build-script-build`");

            if line_stripped.starts_with("Running `") {
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
                    // do nothing: this is an unrelated line of output that we don't care about.
                    original_multiline.clear(); // = String::from(&original_line);
                }
            }

            // In the above xargo command, we added a verbose argument to capture the commands issued from xargo/cargo to rustc. 
            // But if the user didn't ask for that, then we shouldn't print that verbose output here. 
            // Verbose output lines start with "Running `", "+ ", or "[".
            let should_print = |stripped_line: &str| {
                verbose_count > 0 ||  // print everything if verbose
                (
                    // print only "Compiling" and warning/error lines if not verbose
                    !stripped_line.starts_with("+ ")
                    && !stripped_line.starts_with("[")
                    && !stripped_line.starts_with("Running `")
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

