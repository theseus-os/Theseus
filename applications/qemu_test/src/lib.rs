//! An automated test runner.
//!
//! The application assumes it is running in a QEMU virtual machine and exits
//! from QEMU with different exit codes depending on whether the tests passed or
//! failed.

#![no_std]

use alloc::{
    boxed::Box,
    string::String,
    vec::Vec,
};

use app_io::{
    print,
    println,
};
use path::{
    Path,
    PathBuf,
};
use qemu_exit::{
    QEMUExit,
    X86,
};
use task::{
    ExitValue,
    KillReason,
};

extern crate alloc;

static QEMU_EXIT_HANDLE: X86 = X86::new(0xf4, 0x11);

pub fn main(_: Vec<String>) -> isize {
    task::set_kill_handler(Box::new(|_| {
        QEMU_EXIT_HANDLE.exit_failure();
    }))
    .unwrap();

    let dir = task::get_my_current_task()
        .map(|t| t.get_namespace().dir().clone())
        .expect("couldn't get namespace dir");

    let object_files = dir.lock().list();

    let test_paths = object_files
        .into_iter()
        .filter_map(|file_name| {
            if file_name.starts_with("test_") {
                // We must release the lock prior to calling `get_absolute_path` to avoid
                // deadlock.
                let file = dir.lock().get_file(file_name.as_ref()).unwrap();
                let path = file.lock().get_absolute_path();
                Some((file_name, PathBuf::from(path)))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let total = test_paths.len();
    println!("running {} tests", total);

    let mut num_ignored = 0;
    let mut num_failed = 0;

    for (file_name, path) in test_paths.into_iter() {
        print!("test {} ... ", path);
        if ignore(&file_name) {
            num_ignored += 1;
            println!("ignored");
        } else {
            match run_test(&path) {
                Ok(_) => println!("ok"),
                Err(_) => {
                    num_failed += 1;
                    println!("failed");
                }
            }
        }
    }

    let result_str = if num_failed > 0 { "failed" } else { "ok" };
    let num_passed = total - num_failed;
    println!(
        "test result: {result_str}. {num_passed} passed; {num_failed} failed; {num_ignored} \
         ignored",
    );

    if num_failed == 0 {
        QEMU_EXIT_HANDLE.exit_success();
    } else {
        QEMU_EXIT_HANDLE.exit_failure();
    }
}

#[allow(clippy::result_unit_err)]
pub fn run_test(path: &Path) -> Result<(), ()> {
    match spawn::new_application_task_builder(path, None)
        .unwrap()
        .argument(Vec::new())
        .spawn()
        .unwrap()
        .join()
        .unwrap()
    {
        ExitValue::Completed(status) => match status.downcast_ref::<isize>() {
            Some(0) => Ok(()),
            _ => Err(()),
        },
        ExitValue::Killed(KillReason::Requested) => unreachable!(),
        ExitValue::Killed(KillReason::Panic(_)) => Err(()),
        ExitValue::Killed(KillReason::Exception(_)) => Err(()),
    }
}

fn ignore(name: &str) -> bool {
    const IGNORED_TESTS: [&str; 3] = [
        // `test_libc` requires extra Make commands to run.
        "test_libc",
        // `test_panic` panics on success, which isn't easily translatable to
        // `ExitValue::Completed(0)`.
        "test_panic",
        // TODO: Remove
        // `test_channel` has a bug that causes deadlock.
        "test_channel",
    ];
    for test in IGNORED_TESTS {
        if name.starts_with(test) {
            return true;
        }
    }
    false
}
