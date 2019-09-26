#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;

extern crate device_manager;
#[macro_use] extern crate log;
extern crate fat32;
extern crate ata;
extern crate storage_device;
extern crate spin;
extern crate fs_node;
extern crate getopts;
extern crate path;
extern crate task;

use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::sync::{Arc, Weak};
use spin::Mutex;
use fs_node::{File, Directory, FileOrDir, FsNode};
use fat32::{root_dir, PFSDirectory};
use path::Path;
use getopts::Options;

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {

    // Some code that was here when this was a program that I intended to do a mount.
    // I would probably remove it but it's potentially convenient albeit quite trivial code.
    // let mut opts = Options::new();
    // opts.optflag("h", "help", "print this help menu");

    // Attempts to mount code as described in docs for fat32 crate.
    // Mounts to directory given in args.
    // let matches = match opts.parse(&args) {
    //     Ok(m) => m,
    //     Err(_f) => {
    //         println!("{}", _f);
    //         return -1; 
    //     }
    // };
    
    // Verify we have a path to mount to:
    // if matches.free.is_empty() {
    //     println!("need path to mount");
    //     return -1;
    // }
    
    // let path = Path::new(matches.free[0].to_string());
    
    // let taskref = match task::get_my_current_task() {
    //     Some(t) => t,
    //     None => {
    //         println!("failed to get current task");
    //         return -1;
    //     }
    // };
    
    // grabs the current working directory pointer; this is scoped so that we drop the lock on the "cd" task
    // let curr_wd = {
    //     let locked_task = taskref.lock();
    //     let curr_env = locked_task.env.lock();
    //     Arc::clone(&curr_env.working_dir)
    // };
    
    println!("Trying to mount a fat32 drive");
    if let Some(controller) = storage_manager::STORAGE_CONTROLLERS.lock().iter().next() {
        for sd in controller.lock().devices() {
            println!("Got a drive");
            match fat32::init(sd) {
                Ok(fatfs) => {
                    let fs = Arc::new(Mutex::new(fatfs));
                    // TODO if we change the root dir creation approach this will also change.
                    // Take as read only for now?
                    let root_dir = root_dir(fs.clone()).unwrap();
                    
                    // Recursively print files and directories.
                    print_dir(&root_dir);
                    //println!("{:?}", root_dir.list());
                    /*
                    // Reaches the root dir and able to go through each of the entries in the root folder using the next_entry()
                    // but next_entry should not be used to go through a folder because it mutates the folder
                    let de = root_dir.next_entry().unwrap();
                    println!("the name of the next entry is: {:?}", de.name);
                    println!("the name of the next_entry is: {:?}", core::str::from_utf8(&de.name));

                    let de = root_dir.next_entry().unwrap();
                    println!("the name of the next entry is: {:?}", de.name);
                    println!("the name of the next_entry is: {:?}", core::str::from_utf8(&de.name));

                    root_dir.get("test");

                    // Uses the path provided and reads the bytes of the file otherwise returns 0 if file can't be found
                    // The path can be in the format of /hello/poem.txt or \\hello\\poem.txt
                    let path = format!("\\hello\\poem.txt"); // works for subdirectories and files that span beyond a single cluster
                    
                    // This open function create a file structure based on the path if it is found
                    match fat32::open(fs.clone(), &path) {
                        Ok(f) => {
                            debug!("file size:{}", f.size);
                            let mut bytes_so_far = 0;
                            
                            // the buffer provided must be a multiple of the cluster size in bytes, so if the cluster is 8 sectors
                            // the buffer must be a multiple of 8*512 (4096 bytes)
                            let mut data: [u8; 4096*2] = [0;4096*2];
                            
                            match f.read(&mut data, 0) {
                                Ok(bytes) => {
                                    bytes_so_far += bytes;
                                }
                                Err(_er) => panic!("the file failed to read"),
                                }
                            ;
                            debug!("{:X?}", &data[..]);
                            debug!("{:?}", core::str::from_utf8(&data));

                            println!("bytes read: {}", bytes_so_far);
                        }
                        Err(_) => println!("file doesnt exist"),
                            }

                    let path2 = format!("\\test");
                    let file2 = fat32::open(fs.clone(), &path2);
                    println!("name of second file is: {}", file2.unwrap().name);
                    */
                }
                
                Err(_) => {
                    1;
                }
            }
        }
    }
    0
}

fn print_dir(d : &dyn Directory) {
    let entries = d.list();
    println!("Printing directory: {:?}: {:} entries.", d.get_name(), entries.len());

    for entry in entries {
        let node = match d.get(&entry) {
            Some(node) => node,
            None => {
                debug!("Couldn't get entry {:}", entry);
                continue;
            }
        };

        // Don't need to want to get "dot" entries (or else we end up in a loop).
        if node.get_name().chars().nth(0).unwrap_or('.') == '.' {
            continue;
        }

        match node {
            FileOrDir::File(f) => {
                let f_locked: &dyn File = &(*f.lock()); // This looks pretty horrible, but it seems legitimate.
                print_file(f_locked);
            },
            FileOrDir::Dir(d) => {
                let d_locked: &dyn Directory = &(*d.lock()); // This looks pretty horrible, but it seems legitimate.
                print_dir(d_locked);
            }
        }
    }

    println!("Done printing directory: {:?}", d.get_name())
}

// TODO this doesn't work yet. I think my issue is that there's not easy way to get partialEq for the refs get returns since they aren't sized.
// We'd need some sort of type parameter to work around this I think. But I'm a bit shaky on doing that.
// See if we can get a child multiple times and then try to compare them to see if they're both valid and not the same.
fn check_singleton(d : &PFSDirectory) {

    let entries = d.list();
    //println!("Printing directory: {:?}: {:} entries.", d.get_name(), entries.len());

    for entry in entries {
        let node = match d.get(&entry) {
            Some(node) => node,
            None => {
                debug!("Couldn't get entry {:}", entry);
                continue;
            }
        };

        // Don't need to want to get "dot" entries (or else we end up in a loop).
        if node.get_name().chars().nth(0).unwrap_or('.') == '.' {
            continue;
        }

        let node2 = match d.get(&entry) {
            Some(node) => node,
            None => {
                debug!("Couldn't get entry twice {:}", entry);
                continue;
            }
        };

        // Really dumb code here, but I 
        match (node, node2) {
            (FileOrDir::File(f), FileOrDir::File(f2)) => {
                
            },
            (FileOrDir::Dir(d), FileOrDir::Dir(d2)) => {

            },
            (_,_) => {
                println!("Entries don't match in dir/file type.")
            }
        }

        // Compare node and node2 and ensure that they are the same (since they're both )
    }

    //println!("Done checking singleton for directory: {:?}", d.get_name())
    println!("Check singleton not yet working");
    return;
}


fn print_file(f: &dyn File) {
    const SECTOR_SIZE : usize = 512; // FIXME not really a constant here.

    println!("Printing file: {:?}, {:} bytes.", f.get_name(), f.size());
    // Now print the first 512 bytes. Might be shorter than this many bytes.
    let mut data = [0; SECTOR_SIZE];
    let mut pos = 0;

    let size = f.size();

    let mut bytes_read = match f.read(&mut data, pos) {
        Ok(bytes) => bytes,
        Err(_) => {
            println!("Failed to read from file");
            return;
        }
    };
    pos += bytes_read;

    print_data(&data);

    // Read until the end of file.
    while bytes_read == size {
        bytes_read += match f.read(&mut data, pos) {
            Ok(bytes) => bytes,
            Err(_) => {
                println!("Failed to read from file");
                return;
            }
        };
        pos += bytes_read;
    }

    print_data(&data);
    println!("EOF");
}

// TODO I'd like for this to behave the like the cat utility on Linux but I'm not very familiar with that functionality.
fn print_data(data: &[u8]) {
    println!("{:?}", data);
}