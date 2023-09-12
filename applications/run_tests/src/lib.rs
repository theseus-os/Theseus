#![no_std]

use alloc::{string::String, vec::Vec};

use app_io::{print, println};
use path::Path;
use task::{ExitValue, KillReason};

extern crate alloc;

pub fn main(_: Vec<String>) -> isize {
    let dir = task::get_my_current_task()
        .map(|t| t.get_namespace().dir().clone())
        .expect("couldn't get namespace dir");

    let mut num_ignored = 0;
    let object_files = dir.lock().list();

    let test_paths = object_files
        .into_iter()
        .filter_map(|file_name| {
            if file_name.starts_with("test_") {
                if ignore(&file_name) {
                    num_ignored += 1;
                    None
                } else {
                    // We must release the lock prior to calling `get_absolute_path` to avoid
                    // deadlock.
                    let file = dir.lock().get_file(file_name.as_ref()).unwrap();
                    let path = file.lock().get_absolute_path();
                    Some(Path::new(path))
                }
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    println!("d");

    let total = test_paths.len();
    println!("running {} tests", total);

    let mut num_failed = 0;
    for path in test_paths.into_iter() {
        print!("test {} ... ", path);
        match run_test(path) {
            Ok(_) => println!("ok"),
            Err(_) => {
                num_failed += 1;
                println!("failed");
            }
        }
    }

    let result_str = if num_failed > 0 { "failed" } else { "ok" };
    let num_passed = total - num_failed;
    println!(
        "test result: {result_str}. {num_passed} passed; {num_failed} failed; {num_ignored} \
         ignored",
    );

    num_failed as isize
}

pub fn run_test(path: Path) -> Result<(), ()> {
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
            Some(_) => {
                panic!("test failed");
                Err(())
            }
            None => {
                panic!("test did not return isize");
                Err(())
            }
        },
        ExitValue::Killed(KillReason::Requested) => unreachable!(),
        ExitValue::Killed(KillReason::Panic(info)) => {
            todo!("test panicked");
            Err(())
        }
        ExitValue::Killed(KillReason::Exception(code)) => {
            todo!("test triggered an exception");
            Err(())
        }
    }
}

fn ignore(name: &str) -> bool {
    const IGNORED_TESTS: [&str; 2] = ["test_libc", "test_panic"];
    for test in IGNORED_TESTS {
        if name.starts_with(test) {
            return true;
        }
    }
    false
}
