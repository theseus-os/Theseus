//! This application serves as a testbed that runs libc tests.

#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;
#[macro_use] extern crate log;
extern crate cstr_core;

#[macro_use] mod test;

use test::TestResult;
use alloc::string::String;
use cstr_core::CString;

fn test_strchr() -> TestResult {
    let s = "some string";
    let c = s.chars().nth(s.len() / 2).unwrap();

    let ptr = unsafe { libc::string::strchr(s.as_ptr().cast(), c as libc::c_int) };

    if c != unsafe { (*ptr as u8) as char } {
        return Err(String::from("Did not find the right character"));
    }

    Ok(())
}

fn test_files() -> TestResult {
    let f = String::from("file.txt");
    let content = CString::new("file contents").unwrap();

    // Open write file
    let write_file = unsafe {
        libc::stdio::fopen(f.clone().as_ptr().cast(), "w".as_ptr().cast())
    };

    if write_file.is_null() {
        return Err(format!("Could not open {}", stringify!(write_file)));
    }

    info!("test_libc::test_files(): Successfully opened file {}", stringify!(write_file));

    // Write content to file
    let content_size = content.as_bytes_with_nul().len();
    let writ =
        unsafe {
            libc::stdio::fwrite(content.clone().as_ptr().cast(), 1, content_size, write_file)
        };

    if writ != content_size {
        return Err(format!("Tried to write {} bytes to {}, only wrote {}",
                           content_size,
                           stringify!(write_file),
                           writ
        ));
    }

    info!("test_libc::test_files(): Successfully wrote to {}", stringify!(write_file));

    // Open read file before closing write file to avoid deletion
    let read_file = unsafe {
        libc::stdio::fopen(f.clone().as_ptr().cast(), "r".as_ptr().cast())
    };

    if read_file.is_null() {
        return Err(format!("Could not open {}", stringify!(read_file)));
    }

    info!("test_libc::test_files(): Successfully opened file {}", stringify!(read_file));

    // Close write file
    if unsafe { libc::stdio::fclose(write_file) } != 0 {
        return Err(format!("Failed to close file '{}'", stringify!(write_file)));
    }

    info!("test_libc::test_files(): Successfully closed {}", stringify!(write_file));

    // Read content from file
    let mut buf = vec![0; content_size];

    let read =
        unsafe {
            libc::stdio::fread(buf.as_mut_ptr().cast(), 1, content_size, read_file)
        };

    if read != content_size {
        return Err(format!("Tried to read {} bytes from {}, only read {}",
                           content_size,
                           stringify!(read_file),
                           read
        ));
    }

    if buf.last() != Some(&0) {
        return Err(format!("The read string is not null-terminated"));
    }

    buf.pop();
    let string = unsafe { CString::from_vec_unchecked(buf) };

    if string != content {
        return Err(format!("Wrote '{}' to file, but read '{}' instead",
                           content.to_str().unwrap(),
                           string.to_str().unwrap()));
    }

    info!("test_libc::test_files(): Successfully read '{}' from {}",
          string.into_string().unwrap(), stringify!(read_file));

    // Close read file
    if unsafe { libc::stdio::fclose(read_file) } != 0 {
        return Err(format!("Failed to close file '{}'", stringify!(read_file)));
    }

    info!("test_libc::test_files(): Successfully closed {}", stringify!(read_file));

    // Check if file was deleted after closing
    if !unsafe { libc::stdio::fopen(f.clone().as_ptr().cast(), "r".as_ptr().cast()) }
        .is_null() {
        return Err(format!("File not deleted after closing"));
    }

    Ok(())
}

pub fn main() -> isize {
    testbed!(
        test!(test_files),
        test!(test_strchr)
    )
}
