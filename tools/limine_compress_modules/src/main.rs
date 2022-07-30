extern crate getopts;
extern crate lz4_flex;

use lz4_flex::block::compress_prepend_size;
use getopts::Options;
use std::fs::read;
use std::fs::write;
use std::process;
use std::env;

fn main() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();

    let mut opts = Options::new();
    opts.optopt("o", "", "set compressed file path", "OUTPUT_PATH");
    opts.optopt("i", "", "set uncompressed file path", "INPUT_PATH");
    opts.optflag("h", "help", "print this help menu");

    let matches = opts.parse(&args[1..]).map_err(|e| e.to_string())?;

    if matches.opt_present("h") {
        let brief = format!("Usage: cargo run -- [options]");
        print!("{}", opts.usage(&brief));
        process::exit(0);
    }

    let input_path = matches.opt_str("i")
        .ok_or(String::from("failed to match input file argument."))?;
    let output_path = matches.opt_str("o")
        .ok_or(String::from("failed to match output file argument."))?;

    let input = read(input_path).ok()
        .ok_or(String::from("failed to read input file"))?;

    let output = compress_prepend_size(&input);

    write(output_path, output).ok()
        .ok_or(String::from("failed to write to output file"))?;

    Ok(())
}
