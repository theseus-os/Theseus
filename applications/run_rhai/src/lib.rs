//! A frontend for running Rhai scripts or an interactive Rhai REPL shell.

#![no_std]

extern crate alloc;

use alloc::{string::{String, ToString}, vec::Vec, format};
use app_io::println;
use getopts::{Matches, Options};
use path::Path;
use rhai::{
    packages::{Package, CorePackage, BasicStringPackage},
    Engine,
};
use spin::Once;

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &str = "Usage: run_rhai [SCRIPT...]
Runs Rhai as an interactive REPL shell session, if no SCRIPTs are provided.
If SCRIPT paths are provided, each SCRIPT will be run sequentially.";


static QUIET: Once<bool> = Once::new();
macro_rules! quiet {
    () => (QUIET.get() == Some(&true));
}

static KEEP_GOING: Once<bool> = Once::new();
macro_rules! keep_going {
    () => (KEEP_GOING.get() == Some(&true));
}

pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help",             "print this help menu");
    opts.optflag("q", "quiet",            "silence output to stdio");
    opts.optflag("",  "keep-going",       "continue executing SCRIPTs even if an error occurs.");
    // opts.optflag("v", "verbose",          "enable verbose output");
    

    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(opts);
            return -1; 
        }
    };

    if matches.opt_present("h") {
        print_usage(opts);
        return 0;
    }

    QUIET.call_once(|| matches.opt_present("q"));
    KEEP_GOING.call_once(|| matches.opt_present("keep-going"));

    match rmain(matches) {
        Ok(_) => 0,
        Err(e) => {
            println!("Error:\n{}", e);
            -1
        }    
    }
}


fn rmain(matches: Matches) -> Result<(), String> {
    // This is a raw engine with no loaded packages,
    // so it can't really do much besides basic math.
    let mut engine = Engine::new_raw();
    CorePackage::new().register_into_engine(&mut engine);
    BasicStringPackage::new().register_into_engine(&mut engine);

    if matches.free.is_empty() {
        run_interactive(engine)
    } else {
        run_scripts(engine, &matches.free)
    }
}


fn run_interactive(_engine: Engine) -> Result<(), String> {
    Err("Rhai interactive scripting isn't yet complete".to_string())

    /*
    println!("Starting interactive Rhai session:\n>>> ");

    let stdin = app_io::stdin().map_err(ToString::to_string)?;
    let stdout = app_io::stdout().map_err(ToString::to_string)?;
    let mut buf = [0u8; 256];
    let mut 
    loop {
        let cnt = stdin.read(&mut buf).map_err(|e|
            format!("Failed to read stdin: {:?}", e)
        )?;
        if cnt == 0 { break; }
        stdout.write_all(&buf[0..cnt]).map_err(|e|
            format!("Failed to write to stdout: {:?}", e)
        )?;

    }
    println!("Rhai returned {:?}", result);
    */
}

fn run_scripts(engine: Engine, paths: &Vec<String>) -> Result<(), String> {
    let Ok(cwd) = task::with_current_task(|t| t.get_env().lock().working_dir.clone()) else {
        return Err("failed to get current task".to_string());
    };

    for path in paths {
        let path = Path::new(path.to_string());
        let Some(file) = path.get_file(&cwd) else {
            let err_msg = format!("Error: couldn't open file at '{}'", path);
            if !quiet!() { println!("{}", err_msg); }
            if keep_going!() { continue; } else { return Err(err_msg); }
        };
        let mut file_locked = file.lock();
        let file_size = file_locked.len();
        let mut string_slice_as_bytes = alloc::vec![0; file_size];
        
        let _num_bytes_read = match file_locked.read_at(&mut string_slice_as_bytes,0) {
            Ok(num) => num,
            Err(e) => {
                let err_msg = format!("Failed to read '{}', error {:?}", path, e);
                if !quiet!() { println!("{}", err_msg); }
                if keep_going!() { continue; } else { return Err(err_msg); }
            }
        };
        let script_string = match core::str::from_utf8(&string_slice_as_bytes) {
            Ok(string_slice) => string_slice,
            Err(utf8_err) => {
                let err_msg = format!("File {} was not a valid UTF-8 text file: {}", path, utf8_err);
                if !quiet!() { println!("{}", err_msg); }
                if keep_going!() { continue; } else { return Err(err_msg); }
            }
        };
        let rhai_result = engine.run(&script_string);
        match rhai_result {
            Ok(_) => {
                if !quiet!() { println!("Successfully ran script at '{}'", path); }
            }
            Err(e) => {
                let err_msg = format!("Error running script at '{}': {:?}", path, e);
                if !quiet!() { println!("{}", err_msg); }
                if keep_going!() { continue; } else { return Err(err_msg); }
            }
        }
    }

    Ok(())
}
