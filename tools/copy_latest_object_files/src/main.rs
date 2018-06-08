//! Copies object files from one directory to another,
//! taking the latest-modified one out of any duplicates (excluding hashes on the end).
#![feature(assoc_unix_epoch)]

extern crate getopts;
extern crate filetime;

use getopts::Options;
use std::fs;
use std::process;
use std::env;
use std::ffi::OsStr;
use std::path::PathBuf;

use filetime::{FileTime, set_file_times};

struct FileRecord {
    short_name: String,
    access_time: FileTime, 
    modification_time: FileTime,
    file_path: PathBuf,
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut opts = Options::new();
    opts.optflag("v", "verbose", "print input file path to output file path for every file that is copied");
    opts.optflag("h", "help", "print this help menu");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => { m }
        Err(f) => { panic!(f.to_string()) }
    };

    let (input_dir, output_dir) = match matches.free.len() {
        0=>{ eprintln!("No input or output directory");
            process::exit(-1);
        },
        1=>{ eprintln!("No output directory");
            process::exit(-1);
        },
        2=>{ (matches.free[0].clone(), matches.free[1].clone()) }, 
        _=>{ eprintln!("Too many arguments entered");
            process::exit(-1);
        },
    };

    let mut file_list: Vec<FileRecord> = Vec::new();
    let mut unique_source_files: Vec<String> = Vec::new();

    if let Ok(input_dir) = fs::read_dir(input_dir) {
        for file in input_dir {
            if let Ok(file) = file {
                let mut file_path = file.path();
                if Some("o") == file_path.extension().and_then(OsStr::to_str) {
                    let short_name = file_name_until_dash(& mut file_path);
                    let metadata = file.metadata().unwrap();
                    let access_time = FileTime::from_last_access_time(&metadata);
                    let modification_time = FileTime::from_last_modification_time(&metadata);
                    
                    // sort by modification_time for each short_name
                    if !unique_source_files.contains(&short_name) {
                        unique_source_files.push(short_name.clone());
                        file_list.push(FileRecord{ short_name: short_name, access_time: access_time, 
                        modification_time: modification_time, file_path: file_path });
                    }
                    else {
                        for dup_file in file_list.iter_mut() {
                            if dup_file.short_name == short_name && dup_file.modification_time < modification_time {
                                dup_file.modification_time = modification_time;
                                dup_file.file_path = file_path.clone();
                            }
                        }
                    }
                }
            }
            else {
                eprintln!("Invalid file");
                process::exit(-1);
            }       
        }
    }
    else{
        eprintln!("Invalid directory");
        process::exit(-1);
    }

    /* // TEST printing files in file_list
    for file in file_list.iter() {
        println!("full_name {}", file.full_name);
    } */

    // copy files to output directory
    for file in file_list.iter() {
        let mut output_path = PathBuf::from(output_dir.clone());
        output_path.push(format!("{}{}", "__k_", file.short_name.clone()));
        output_path.set_extension("o");
        
        if matches.opt_present("v") {
            println!("{:?} --> {:?}", file.file_path, output_path)
        }
        if fs::copy(file.file_path.clone(), output_path.as_path()).is_err() {
            eprintln!("Failed to copy file");
            process::exit(-1);
        }  
        
        if set_file_times(output_path, file.access_time, file.modification_time).is_err() {
            eprintln!("Failed to set file time");
            process::exit(-1);
        }

    }
    process::exit(0);
}

fn file_name_until_dash(file_path: & mut PathBuf) -> String {
    let full_name = String::from(file_path.file_name().unwrap().to_str().unwrap());
    let short_name = String::from(full_name.split("-").collect::<Vec<&str>>()[0]);
    short_name
}

