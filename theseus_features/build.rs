#![feature(path_try_exists)]

use core::panic;
use std::{fs, process::Command};

fn main() {
    clone_wasmtime_repo();
}

static REPO_URL: &str = "https://github.com/theseus-os/wasmtime.git";
static REPO_BRANCH: &str = "theseus";

static ROOT_PORTS_WASMTIME_DIR: &str = "ports/wasmtime";


/// Clone the wasmtime repo to the proper location above.
/// 
/// Any errors here should be translated to panics to ensure the build process fails.
fn clone_wasmtime_repo() {
    let path_to_wasmtime_repo = std::env::current_dir().unwrap()
        .parent().unwrap()
        .join(ROOT_PORTS_WASMTIME_DIR);

    if fs::try_exists(&path_to_wasmtime_repo).unwrap() {
        eprintln!("Note: the path {:?} already exists, not cloning the 'wasmtime' repo.", path_to_wasmtime_repo);
        return;
    } else {
        eprintln!("Note: the path {:?} did not exist, proceeding to clone the 'wasmtime' repo.", path_to_wasmtime_repo);
    }

    let status = Command::new("git")
        .arg("clone")
        .arg("--depth").arg("1")
        // .arg("--recursive")
        .arg(REPO_URL)
        .arg("-b")
        .arg(REPO_BRANCH)
        .arg(&path_to_wasmtime_repo)
        .status()
        .expect("Failed to execute 'git' command");

    if status.success() {
        eprintln!("Successfully cloned wasmtime git repo to {:?}", path_to_wasmtime_repo);
    } else {
        panic!("git clone of wasmtime repo failed with error code {:?}", status.code());
    }

}