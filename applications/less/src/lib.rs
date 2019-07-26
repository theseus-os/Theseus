#![no_std]

#[macro_use] extern crate application_io;
#[macro_use] extern crate alloc;
extern crate task;
extern crate getopts;
extern crate path;
extern crate fs_node;
extern crate terminal_map;
extern crate keycodes_ascii;
extern crate libterm;
extern crate spin;

use keycodes_ascii::Keycode;
use core::str;
use alloc::{
    vec::Vec,
    string::{String, ToString},
    sync::Arc,
};
use getopts::Options;
use path::Path;
use fs_node::FileOrDir;
use alloc::collections::BTreeMap;
use libterm::Terminal;
use spin::Mutex;

/// The metadata for each line in the file.
struct LineSlice {
    /// The starting index in the String for a line. (inclusive)
    start: usize,
    /// The ending index in the String for a line. (exclusive)
    end: usize
}

/// Read the whole file to a String.
fn get_content_string(file_path: String) -> Result<String, String> {
    let taskref = match task::get_my_current_task() {
        Some(t) => t,
        None => {
            return Err("failed to get current task".to_string());
        }
    };

    // grabs the current working directory pointer; this is scoped so that we drop the lock on the task as soon as we get the working directory pointer
    let curr_wr = {
        let locked_task = taskref.lock();
        let curr_env = locked_task.env.lock();
        Arc::clone(&curr_env.working_dir)
    };
    let path = Path::new(file_path);
    
    // navigate to the filepath specified by first argument
    match path.get(&curr_wr) {
        Some(file_dir_enum) => { 
            match file_dir_enum {
                FileOrDir::Dir(directory) => {
                    Err(format!("{:?} is a directory, cannot 'less' non-files.", directory.lock().get_name()))
                }
                FileOrDir::File(file) => {
                    let file_locked = file.lock();
                    let file_size = file_locked.size();
                    let mut string_slice_as_bytes = vec![0; file_size];
                    
                    let _num_bytes_read = match file_locked.read(&mut string_slice_as_bytes,0) {
                        Ok(num) => num,
                        Err(e) => {
                            return Err(format!("Failed to read {:?}, error {:?}",
                                               file_locked.get_name(), e).to_string())
                        }
                    };
                    let read_string = match str::from_utf8(&string_slice_as_bytes) {
                        Ok(string_slice) => string_slice,
                        Err(utf8_err) => {
                            return Err(format!("File {:?} was not a printable UTF-8 text file: {}",
                                               file_locked.get_name(), utf8_err).to_string())
                        }
                    };
                    Ok(read_string.to_string())
                }
            }
        },
        _ => {
            Err(format!("Couldn't find file at path {}", path).to_string())
        }
    }
}

/// This function parses the text file. It scans through the whole file and records the string slice
/// for each line. This function has full UTF-8 support, which means that the case where a single character
/// occupies multiple bytes are well considered. The slice index returned by this function is guaranteed
/// not to cause panic.
fn parse_content(content: &String) -> Result<BTreeMap<usize, LineSlice>, &'static str> {
    // Get the width and height of the terminal screen.
    let (width, _height) = terminal_map::get_terminal_or_default()?.lock().get_width_height();

    // Record the slice index of each line.
    let mut map: BTreeMap<usize, LineSlice> = BTreeMap::new();
    // Number of the current line.
    let mut cur_line_num: usize = 0;
    // Number of characters in the current line.
    let mut char_num_in_line: usize = 0;
    // Starting index in the String of the current line.
    let mut line_start_idx: usize = 0;
    // The previous character during the iteration. Set '\0' as the initial value since we don't expect
    // to encounter this character in the beginning of the file.
    let mut previous_char: char = '\0';

    // Iterate through the whole file.
    // `c` is the current character. `str_idx` is the index of the first byte of the current character.
    for (str_idx, c) in content.char_indices() {
        // When we need to begin a new line, record the previous line in the map.
        if char_num_in_line == width || previous_char == '\n' {
            map.insert(cur_line_num, LineSlice{ start: line_start_idx, end: str_idx });
            char_num_in_line = 0;
            line_start_idx = str_idx;
            cur_line_num += 1;
        }
        char_num_in_line += 1;
        previous_char = c;
    }
    map.insert(cur_line_num, LineSlice{ start: line_start_idx, end: content.len() });

    Ok(map)
}

/// Display part of the file (may be whole file if the file is short) to the terminal, starting
/// at line number `line_start`.
fn display_content(content: &String, map: &BTreeMap<usize, LineSlice>,
                   line_start: usize, terminal: &Arc<Mutex<Terminal>>) {
    // Get exclusive control of the terminal. It is locked through the whole function to
    // avoid the overhead of locking it multiple times.
    let mut locked_terminal = terminal.lock();

    // Calculate the last line to display. Make sure we don't extend over the end of the file.
    let (_width, height) = locked_terminal.get_width_height();
    let mut line_end: usize = line_start + height;
    if line_end > map.len() {
        line_end = map.len();
    }

    // Refresh the terminal with the lines we've selected. Unwrap is used here since the keys
    // passed to the map are guaranteed to be valid.
    locked_terminal.clear();
    locked_terminal.print_to_terminal(
        content[map.get(&line_start).unwrap().start..map.get(&(line_end-1)).unwrap().end].to_string()
    );
    locked_terminal.refresh_display(0);
}

/// Handle user keyboard strikes and perform corresponding operations.
fn event_handler_loop(content: &String, map: &BTreeMap<usize, LineSlice>) -> Result<(), &'static str> {

    // Get a copy of the terminal pointer. The terminal is *not* locked here.
    let terminal = terminal_map::get_terminal_or_default()?;

    // Display the beginning of the file.
    let mut line_start: usize = 0;
    display_content(content, map, 0, &terminal);

    // Handle user keyboard strikes.
    loop {
        match application_io::get_keyboard_event() {
            Ok(Some(keyevent)) => {
                match keyevent.keycode {
                    // Quit the program on "Q".
                    Keycode::Q => {
                        let mut locked_terminal = terminal.lock();
                        locked_terminal.clear();
                        locked_terminal.refresh_display(0);
                        return Ok(());
                    },
                    // Scroll down a line on "Down".
                    Keycode::Down => {
                        if line_start + 1 < map.len() {
                            line_start += 1;
                        }
                        display_content(content, map, line_start, &terminal);
                    },
                    // Scroll up a line on "Up".
                    Keycode::Up => {
                        if line_start > 0 {
                            line_start -= 1;
                        }
                        display_content(content, map, line_start, &terminal);
                    }
                    _ => {}
                }
            },
            Err(e) => {
                println!("{}", e);
            },
            _ => {}
        }
    }
}


#[no_mangle]
pub fn main(args: Vec<String>) -> isize {

    // Set and parse options.
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
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
    if matches.free.is_empty() {
        print_usage(opts);
        return 0;
    }

    // Read the whole file to a String.
    let content = match get_content_string(matches.free[0].to_string()) {
        Ok(s) => s,
        Err(e) => {
            println!("{}", e);
            return -1;
        }
    };

    // Request the shell to direct keyboard events instead of pushing it
    // to stdin.
    if let Err(e) = application_io::request_kbd_event_forward() {
        println!("{}", e);
        return -1;
    }

    // Turn off the echo of shell, so that it won't print characters to
    // the terminal on keyboard strikes.
    if let Err(e) = application_io::request_no_echo() {
        println!("{}", e);
        return -1;
    }

    // Get it run.
    let map =  match parse_content(&content) {
        Ok(map) => {map},
        Err(e) => {
            println!("{}", e);
            return -1;
        }
    };
    if let Err(e) = event_handler_loop(&content, &map) {
        println!("{}", e);
        return -1;
    }

    return 0;
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &'static str = "Usage: less file
read files";
