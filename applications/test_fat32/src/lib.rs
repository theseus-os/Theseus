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
extern crate root;

use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::sync::{Arc, Weak};
use spin::Mutex;
use fs_node::{File, Directory, FileOrDir, FsNode, DirRef, FileRef};
use fat32::{root_dir, PFSDirectory, RootDirectory};
use root::get_root;
use path::Path;
use getopts::Options;

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {

    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optopt("m", "mount", "Attempt to mount the root directory to DIR.", "DIR");
    opts.optflag("p", "print", "Walks the filesystem and prints the contents.");
    opts.optflag("t", "test", "Look for a fat32 object at /fat32 and print it");

    // Attempts to mount code as described in docs for fat32 crate.
    // Mounts to directory given in args.
    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            info!("{}", _f);
            return -1; 
        }
    };

    if matches.opt_present("h") {
        print_usage(&opts);
        return 0;
    }

    // Dumb test TODO don't use this.
    if matches.opt_present("t") {
        
        let names = get_root().lock().get_dir("fat32").unwrap().lock().list();
        debug!("Found children of /fat32: {:?}", names);
        return 0;
    }    
    
    let taskref = match task::get_my_current_task() {
        Some(t) => t,
        None => {
            info!("failed to get current task");
            return -1;
        }
    };
    
    // grabs the current working directory pointer; this is scoped so that we drop the lock on the "current" task
    let curr_wd = {
        let locked_task = taskref.lock();
        let curr_env = locked_task.env.lock();
        Arc::clone(&curr_env.working_dir)
    };

    
    
    info!("Trying to find a fat32 drive");
    if let Some(controller) = storage_manager::STORAGE_CONTROLLERS.lock().iter().next() {
        for sd in controller.lock().devices() {
            match fat32::init(sd) {
                Ok(fatfs) => {
                    println!("Successfully initialized FAT32 FS for drive");

                    let fs = Arc::new(Mutex::new(fatfs));
                    // TODO if we change the root dir creation approach this will also change.
                    // Take as read only for now?


                    let (name, o_path) = if matches.opt_present("m") {
                        let path = Path::new(matches.opt_str("m").unwrap());

                        let name = path.basename().to_string();
                        (name, Some(path.clone()))
                    } else {
                        ("/".to_string(), None)
                    };

                    let name = "fat32".to_string();

                    //let fat32_root: Arc<Mutex<RootDirectory>> = root_dir(fs.clone(), name.clone()).unwrap();
                    let fat32_root: DirRef = root_dir(fs.clone(), name.clone()).unwrap();
                    
                    // // Recursively print files and directories.
                    if matches.opt_present("p") {
                        print_dir(fat32_root.clone());
                    };

                    // If we have a mount point, then mount.
                    match o_path {
                        // FIXME: path Struct really needs a "strip the last element from path" function.
                        // For now we're just going to hope the path is a basename...
                        Some(path) => {
                            if path.basename() != path.to_string() {
                                warn!("Current mount code doesn't handle full paths");
                            }

                            // FIXME set root parent dir to appropriate value.
                            //let mut curr_dir = curr_wd.lock();
                            //let parent = curr_wd;
                            let parent = get_root();
                            {
                                let mut locked_parent = parent.lock();
                                //let root = Arc::new(Mutex::new(root)); // CAUSES ISSUE FIXME
                                //let root = fat32::make_dir_ref(root);
                                match locked_parent.insert(FileOrDir::Dir(fat32_root.clone())) {
                                    Ok(_) => println!("Successfully mounted fat32 FS"),
                                    Err(_) => println!("Failed to mount fat32 FS"),
                                };

                                // Now let's try a couple simple things:
                                let test_root = locked_parent.get_dir(&name).unwrap();
                                println!("Root directory entries: {:?}", test_root.lock().list());
                            }
                            fat32_root.lock().set_parent_dir(Arc::downgrade(parent));
                        },
                        None => {}
                    };           
                }
                
                Err(_) => {
                    1;
                }
            }
        }
    }
    0
}

fn print_dir(dirref: DirRef) {
    let d = dirref.lock();
    let entries = d.list();
    if entries.len() <= 0 {
        return; // Don't print empty directories to save some time.
    };
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
            FileOrDir::File(fileref) => {
                //let f_locked: &dyn File = &(*f.lock()); // This looks pretty horrible, but it seems legitimate.
                print_file(fileref);
            },
            FileOrDir::Dir(dirref) => {
                //let d_locked: &dyn Directory = &(*d.lock()); // This looks pretty horrible, but it seems legitimate.
                print_dir(dirref);
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


fn print_file(fileref: FileRef) {
    const SECTOR_SIZE : usize = 512; // FIXME not really a constant here.

    let f = fileref.lock();

    println!("Printing file: {:?}, {:} bytes.", f.get_name(), f.size());
    trace!("Printing file {:?},. {} bytes", f.get_name(), f.size());
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
    trace!("Read {} bytes", bytes_read);
    pos += bytes_read;

    print_data(&data);

    // Read until the end of file.
    while pos < size {
        bytes_read = match f.read(&mut data, pos) {
            Ok(bytes) => bytes,
            Err(_) => {
                println!("Failed to read from file");
                return;
            }
        };
        pos += bytes_read;
        trace!("Read {} bytes. New pos {}", bytes_read, pos);
    }

    print_data(&data);
    println!("EOF");
}

// TODO I'd like for this to behave the like the cat utility on Linux but I'm not very familiar with that functionality.
fn print_data(data: &[u8]) {
    println!("{:?}", data);
}

fn print_usage(opts: &getopts::Options) {
    print!("{:?}", opts.usage("Usage: test_fat32 [OPTIONS] [MOUNT POINT]"));
}