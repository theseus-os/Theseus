#![no_std]
#![feature(alloc)]

extern crate alloc;


use alloc::string::String;


pub struct File {
    name: String, 
    filepath: String,
    size: usize, 
    filetype: Option<String>
}

impl File {

    fn read()

}

pub fn open(filepath: String) -> File {
    file = new File();
    return file;
}

