//! Differences crate object files across two different builds of Theseus
//! to see what has changed, for purposes of creating an evolution manifest.

extern crate getopts;
extern crate walkdir;
extern crate qp_trie;
extern crate multimap;

use getopts::Options;
use std::fs;
use std::path::{Path, PathBuf};
use std::io::Write;
use std::process;
use std::env;
use multimap::MultiMap;
use std::string::ToString;
use qp_trie::{
    Trie,
    wrapper::BString,
};
use walkdir::WalkDir;


fn main() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();

    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");

    let matches = opts.parse(&args[1..]).map_err(|e| e.to_string())?;

    if matches.opt_present("h") {
        usage("cargo run -- ", opts);
        return Ok(());
    }

    // Require two directories as input 
    let (old_dir_path, new_dir_path) = match matches.free.len() {
        2 => (&matches.free[0], &matches.free[1]),
        _ => return Err(format!("expected two directories as arguments")),
    };

    
    // A closure that returns a Trie map of (file_name, path) from all of the files in the given directory `dir_path`
    let get_files_in_dir = |dir_path| { 
        WalkDir::new(dir_path)
            .into_iter()
            .filter_map(|res| res.ok())
            .filter(|entry| entry.path().is_file())
            .filter_map(|entry| entry.path().file_name()
                .map(|fname| {
                    (
                        fname.to_string_lossy().to_string().into(), 
                        entry.path().to_path_buf()
                    )
                })
            )
            .collect::<Trie<BString, PathBuf>>()
    };

    
    let old_dir_contents = get_files_in_dir(old_dir_path);
    let new_dir_contents = get_files_in_dir(new_dir_path);


    if false {
        println!("---------------------------- OLD DIR ----------------------------------");
        for (filename, p) in old_dir_contents.iter() {
            println!("{:>50}     {}", filename.as_str(), p.display());
        }
        println!("---------------------------- NEW DIR ----------------------------------");
        for (filename, p) in new_dir_contents.iter() {
            println!("{:>50}     {}", filename.as_str(), p.display());
        }
    }
    

    let replacements = compare_dirs(old_dir_contents, new_dir_contents).map_err(|e| e.to_string())?;
    println!("\nREPLACEMENTS:\n{:?}", replacements);
    Ok(())
}


/// Goes through the contents of each directory to compare each file. 
/// 
/// Returns a mapping of old crate to new crate, meaning that the old crate should be replaced with the new crate. 
/// If the old crate is `None` and the new crate is `Some`, then the new crate is a new addition that does not replace any old crate.
/// If the old crate is `Some` and the new crate is `None`, then the old crate is merely being removed without being replaced.
/// If both the old crate and new crate are `Some`, then the new crate is replacing the old crate.
fn compare_dirs(old_dir_contents: Trie<BString, PathBuf>, new_dir_contents: Trie<BString, PathBuf>) -> std::io::Result<MultiMap<Option<String>, Option<String>>> {
    let mut replacements: MultiMap<Option<String>, Option<String>> = MultiMap::new();

    // First, we go through the new directory and see which files have changed since the old directory
    for (new_filename, new_path) in new_dir_contents.iter() {

        // if the old dir contains an identical file as the new dir, then we diff their actual contents
        if let Some(old_path) = old_dir_contents.get(new_filename) {
            let old_file = fs::read(old_path)?;
            let new_file = fs::read(new_path)?;
            if old_file != new_file {
                println!("{0} -> {0}", new_filename.as_str());
                replacements.insert(Some(new_filename.clone().into()), Some(new_filename.clone().into()));
            }
        }
        // otherwise we try to search the old dir to see if there's a single matching crate that has a different hash
        else {
            let matching_old_crates: Vec<(BString, PathBuf)> = old_dir_contents.iter_prefix_str(crate_name_without_hash(new_filename.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
            match &matching_old_crates[..] {
                [] => {
                    // if empty, there were no matches, so the crate is brand new and should be added but not replace anything. 
                    println!("+ {}", new_filename.as_str());
                    replacements.insert(None, Some(new_filename.clone().into()));
                }
                [(old_filename, _old_path)] => {
                    // If there was one match, it means we updated from an old crate to a new crate of the same name, but the hash changed.
                    // This is the most common scenario.
                    println!("{} -> {}", old_filename.as_str(), new_filename.as_str());
                    replacements.insert(Some(old_filename.clone().into()), Some(new_filename.clone().into()));
                }
                other => {
                    eprintln!("Unsupported: there were multiple old crates that matched the new crate {}:", new_filename.as_str());
                    for (k, _v) in other {
                        eprintln!("\t{}", k.as_str());
                    }
                }
            }

        }
    }


    // Second, we got through the old directory to make sure we didn't miss any files that were present in the old directory but not in the new
    for (old_filename, _old_path) in old_dir_contents.iter() {
        if new_dir_contents.iter_prefix_str(crate_name_without_hash(old_filename.as_str())).next().is_none() {
            println!("- {}", old_filename.as_str());
            replacements.insert(Some(old_filename.clone().into()), None);
        }
    }

    Ok(replacements)
}


fn crate_name_without_hash<'s>(name: &'s str) -> &'s str {
    name.split("-")
        .next()
        .unwrap_or_else(|| name.as_ref())
}




fn usage(program: &str, opts: Options) {
    let mut brief = format!("Usage: {} [options] OLD_DIR NEW_DIR\n", program);
    brief.push_str("Outputs the list of differing files in a format that shows how to change OLD_DIR into NEW_DIR\n");
    println!("{}", opts.usage(&brief));
}


#[test]
fn test() {
    use multimap::MultiMap;

    let mut map: MultiMap<usize, usize> = MultiMap::new();
    map.insert(1,42);
    map.insert(1,1337);
    map.insert(3,2332);
    map.insert(4,1991);

    for (k, values) in map.iter_all() {
        for v in values {
            println!("{}, {}", k, v);
        }
    }

    for iter in map.iter_all() {
        println!("iter: {:?}", iter);
        // println!("key: {:?}, val: {:?}", key, value);
    }
}