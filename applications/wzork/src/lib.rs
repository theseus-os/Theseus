//! zork (source: https://github.com/devshane/zork)
//! about the game: https://en.wikipedia.org/wiki/Zork#Zork_and_Dungeon
//! compiled to WebAssembly with WASI support and run with wasi_interpreter

#![no_std]

extern crate alloc;
extern crate memfs;
extern crate root;
extern crate wasi_interpreter;

use alloc::string::String;
use alloc::vec::Vec;

pub fn main(args: Vec<String>) -> isize {
    // Zork data file
    let zork_dat: Vec<u8> = include_bytes!("dtextc.dat").to_vec();
    let root_dir = root::get_root();
    let zork_dat_file = memfs::MemFile::new(String::from("dtextc.dat"), &root_dir).unwrap();
    zork_dat_file.lock().write(&zork_dat, 0).unwrap();

    // Parse WAT (WebAssembly Text format) into wasm bytecode.
    let wasm_binary: Vec<u8> = include_bytes!("zork.wasm").to_vec();

    wasi_interpreter::execute_binary(wasm_binary, args)
}
