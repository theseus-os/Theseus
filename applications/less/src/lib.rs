//! A text file reader which allows the user using `Up` and `Down` to scroll the screen.
#![no_std]

// FIXME

extern crate alloc;
// extern crate task;
// extern crate getopts;
// extern crate path;
// extern crate fs_node;
// extern crate keycodes_ascii;
// extern crate libterm;
// extern crate spin;
// extern crate app_io;
// extern crate stdio;
// extern crate core2;
// #[macro_use] extern crate log;

// use keycodes_ascii::{Keycode, KeyAction};
// use core::str;
use alloc::{
    vec::Vec,
    string::String,
};
// use getopts::Options;
// use path::Path;
// use fs_node::FileOrDir;
// use alloc::collections::BTreeMap;
// use libterm::Terminal;
// use spin::Mutex;
// use stdio::{StdioWriter, KeyEventQueueReader};
// use core2::io::Write;

// /// The metadata for each line in the file.
// struct LineSlice {
//     /// The starting index in the String for a line. (inclusive)
//     start: usize,
//     /// The ending index in the String for a line. (exclusive)
//     end: usize
// }

// /// Read the whole file to a String.
// fn get_content_string(file_path: String) -> Result<String, String> {
//     let Ok(curr_wd) = task::with_current_task(|t| t.get_env().lock().working_dir.clone()) else {
//         return Err("failed to get current task".to_string());
//     };
//     let path = Path::new(file_path);
    
//     // navigate to the filepath specified by first argument
//     match path.get(&curr_wd) {
//         Some(file_dir_enum) => { 
//             match file_dir_enum {
//                 FileOrDir::Dir(directory) => {
//                     Err(format!("{:?} is a directory, cannot 'less' non-files.", directory.lock().get_name()))
//                 }
//                 FileOrDir::File(file) => {
//                     let mut file_locked = file.lock();
//                     let file_size = file_locked.len();
//                     let mut string_slice_as_bytes = vec![0; file_size];
//                     let _num_bytes_read = match file_locked.read_at(&mut string_slice_as_bytes, 0) {
//                         Ok(num) => num,
//                         Err(e) => {
//                             return Err(format!("Failed to read {:?}, error {:?}",
//                                                file_locked.get_name(), e))
//                         }
//                     };
//                     let read_string = match str::from_utf8(&string_slice_as_bytes) {
//                         Ok(string_slice) => string_slice,
//                         Err(utf8_err) => {
//                             return Err(format!("File {:?} was not a printable UTF-8 text file: {}",
//                                                file_locked.get_name(), utf8_err))
//                         }
//                     };
//                     Ok(read_string.to_string())
//                 }
//             }
//         },
//         _ => {
//             Err(format!("Couldn't find file at path {}", path))
//         }
//     }
// }

// /// This function parses the text file. It scans through the whole file and records the string slice
// /// for each line. This function has full UTF-8 support, which means that the case where a single character
// /// occupies multiple bytes are well considered. The slice index returned by this function is guaranteed
// /// not to cause panic.
// fn parse_content(content: &String) -> Result<BTreeMap<usize, LineSlice>, &'static str> {
//     // Get the width and height of the terminal screen.
//     let (width, _height) = app_io::get_my_terminal().ok_or("couldn't get terminal for `less` app")?
//         .lock()
//         .get_text_dimensions();

//     // Record the slice index of each line.
//     let mut map: BTreeMap<usize, LineSlice> = BTreeMap::new();
//     // Number of the current line.
//     let mut cur_line_num: usize = 0;
//     // Number of characters in the current line.
//     let mut char_num_in_line: usize = 0;
//     // Starting index in the String of the current line.
//     let mut line_start_idx: usize = 0;
//     // The previous character during the iteration. Set '\0' as the initial value since we don't expect
//     // to encounter this character in the beginning of the file.
//     let mut previous_char: char = '\0';

//     // Iterate through the whole file.
//     // `c` is the current character. `str_idx` is the index of the first byte of the current character.
//     for (str_idx, c) in content.char_indices() {
//         // When we need to begin a new line, record the previous line in the map.
//         if char_num_in_line == width || previous_char == '\n' {
//             map.insert(cur_line_num, LineSlice{ start: line_start_idx, end: str_idx });
//             char_num_in_line = 0;
//             line_start_idx = str_idx;
//             cur_line_num += 1;
//         }
//         char_num_in_line += 1;
//         previous_char = c;
//     }
//     map.insert(cur_line_num, LineSlice{ start: line_start_idx, end: content.len() });

//     Ok(map)
// }

// /// Display part of the file (may be whole file if the file is short) to the terminal, starting
// /// at line number `line_start`.
// fn display_content(content: &String, map: &BTreeMap<usize, LineSlice>,
//                    line_start: usize, terminal: &Arc<Mutex<Terminal>>)
//                    -> Result<(), &'static str> {
//     // Get exclusive control of the terminal. It is locked through the whole function to
//     // avoid the overhead of locking it multiple times.
//     let mut locked_terminal = terminal.lock();

//     // Calculate the last line to display. Make sure we don't extend over the end of the file.
//     let (_width, height) = locked_terminal.get_text_dimensions();
//     let mut line_end: usize = line_start + height;
//     if line_end > map.len() {
//         line_end = map.len();
//     }

//     // Refresh the terminal with the lines we've selected.
//     let start_indices = match map.get(&line_start) {
//         Some(indices) => indices,
//         None => return Err("failed to get the byte indices of the first line")
//     };
//     let end_indices = match map.get(&(line_end - 1)) {
//         Some(indices) => indices,
//         None => return Err("failed to get the byte indices of the last line")
//     };
//     locked_terminal.clear();
//     locked_terminal.print_to_terminal(
//         content[start_indices.start..end_indices.end].to_string()
//     );
//     locked_terminal.refresh_display()
// }

// /// Handle user keyboard strikes and perform corresponding operations.
// fn event_handler_loop(content: &String, map: &BTreeMap<usize, LineSlice>,
//                       key_event_queue: &KeyEventQueueReader)
//                       -> Result<(), &'static str> {

//     // Get a reference to this task's terminal. The terminal is *not* locked here.
//     let terminal = app_io::get_my_terminal().ok_or("couldn't get terminal for `less` app")?;

//     // Display the beginning of the file.
//     let mut line_start: usize = 0;
//     display_content(content, map, 0, &terminal)?;

//     // Handle user keyboard strikes.
//     loop {
//         match key_event_queue.read_one() {
//             Some(keyevent) => {
//                 if keyevent.action != KeyAction::Pressed { continue; }
//                 match keyevent.keycode {
//                     // Quit the program on "Q".
//                     Keycode::Q => {
//                         let mut locked_terminal = terminal.lock();
//                         locked_terminal.clear();
//                         return locked_terminal.refresh_display()
//                     },
//                     // Scroll down a line on "Down".
//                     Keycode::Down => {
//                         if line_start + 1 < map.len() {
//                             line_start += 1;
//                         }
//                         display_content(content, map, line_start, &terminal)?;
//                     },
//                     // Scroll up a line on "Up".
//                     Keycode::Up => {
//                         if line_start > 0 {
//                             line_start -= 1;
//                         }
//                         display_content(content, map, line_start, &terminal)?;
//                     }
//                     _ => {}
//                 }
//             },
//             _ => {}
//         }
//     }
// }


pub fn main(_args: Vec<String>) -> isize {

    // // Get stdout.
    // let stdout = match app_io::stdout() {
    //     Ok(stdout) => stdout,
    //     Err(e) => {
    //         error!("{}", e);
    //         return 1;
    //     }
    // };

    // // Set and parse options.
    // let mut opts = Options::new();
    // opts.optflag("h", "help", "print this help menu");
    // let matches = match opts.parse(args) {
    //     Ok(m) => m,
    //     Err(e) => {
    //         error!("{}", e);
    //         print_usage(opts, stdout);
    //         return -1;
    //     }
    // };
    // if matches.opt_present("h") {
    //     print_usage(opts, stdout);
    //     return 0;
    // }
    // if matches.free.is_empty() {
    //     print_usage(opts, stdout);
    //     return 0;
    // }
    // let filename = matches.free[0].clone();

    // if let Err(e) = run(filename) {
    //     error!("{}", e);
    //     return 1;
    // }
    0
}

// fn run(filename: String) -> Result<(), String> {

//     // Acquire key event queue.
//     let key_event_queue = app_io::take_key_event_queue()?;
//     let key_event_queue = (*key_event_queue).as_ref()
//                           .ok_or("failed to take key event reader")?;

//     // Read the whole file to a String.
//     let content = get_content_string(filename)?;

//     // Get it run.
//     let map = parse_content(&content)?;

//     Ok(event_handler_loop(&content, &map, key_event_queue)?)
// }

// fn print_usage(opts: Options, stdout: StdioWriter) {
//     let _ = stdout.lock().write_all(format!("{}\n", opts.usage(USAGE)).as_bytes());
// }

// const USAGE: &'static str = "Usage: less file
// read files";
