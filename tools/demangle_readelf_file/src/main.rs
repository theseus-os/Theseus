//! Takes the output of `readelf -aW` of an object file 
//! and demangles mangled symbol names. 
//! Leaves the original file intact, and prints to stdout 
//! unless a "-o OUTPUT_FILE" argument is given
//! In the output file, each mangled symbol is replaced by 
//! its demangled symbol and the trailing hash value, separated by a space.
//! For example, an input of "_ZN7console4init17h71243d883671cb51E"
//! produces an output of "console::init h71243d883671cb51E".

extern crate rustc_demangle;
extern crate getopts; 

use getopts::Options;
use std::fs::File;
use std::io::prelude::*;
use std::process;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut opts = Options::new();
    opts.optopt("o", "", "set output file path", "OUTPUT_PATH");
    opts.optflag("h", "help", "print help menu");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => { m }
        Err(f) => { panic!(f.to_string()) }
    };

    if matches.opt_present("h") {
        print_usage("cargo run -- ", opts);
        process::exit(0);
    }

    // Require input file 
    let input_file = match matches.free.len() {
        0 => {
            eprintln!("No input file");
            process::exit(-1);
        },
        1 => matches.free[0].clone(), 
        _ => { 
            eprintln!("Too many arguments entered");
            process::exit(-1);
        },
    };


    let mut f = File::open(input_file).expect("input file not found!");
    let mut contents = String::new();
    f.read_to_string(&mut contents).expect("Could not read the input file");
    let mut file_iterator = contents.lines();

    let mut output = String::new();

    while let Some(line) = file_iterator.next() {
        // copy lines into output, until we find the "Symbol table" line
        if !line.starts_with("Symbol table") {
            output.push_str(line); 
            output.push_str("\n");
            continue;
        }
        else {
            // we found the "Symbol table" line
            output.push_str(line); // push that "Symbol table" line
            output.push_str("\n");
            output.push_str(file_iterator.next().unwrap()); // and the next one ("   Num:    Value ...")
            output.push_str("\n");
            break; // we're at the first symbol table entry, continue onto the next part
        }
    }


    // parse each symbol table entry and demangle the names
    for line in file_iterator {
        // println!("line: {}", line);
        // we need to find the mangled symbol in each symtab entry, which always starts with "_ZN"
        if let Some(index) = line.find("_ZN") {
            let (first_half, name_mangled) = line.split_at(index);
            let demangled = demangle_symbol(name_mangled);
            output.push_str(first_half); // no newline after this, since it's just a split line
            output.push_str(&demangled);
            output.push_str("\n");
        }
        // if we cannot find "_ZN", then there wasn't a mangled symbol (it might've been no_mangle)
        else {
            // so just preserve the line as is
            output.push_str(line);
            output.push_str("\n");
        }
        
    }


    // if we had a "-o OUTPUT_FILE" argument, then write the output to that file 
    if matches.opt_present("o") {
        let output_file_path = match matches.opt_str("o") {
            Some(path) => path, 
            _ => process::exit(-1),
        };

        if let Ok(mut file) = File::create(output_file_path) {
            if let Err(e) = file.write(output.as_bytes()) {
                eprintln!("Error writing to output file. Error: {}", e);
                std::process::exit(-1);
            }
        }
    }
    // otherwise write to stdout
    else {
        println!("{}", output);
    }
}


fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} [options] READELF_TEXT", program);
    print!("{}", opts.usage(&brief));
}





fn demangle_symbol(mangled: &str) -> String {
    rustc_demangle::demangle(mangled).to_string()
}
