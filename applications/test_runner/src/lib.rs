#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::fmt::Write;
use task::ExitValue;

struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        exit_qemu(ExitCode::Failed);
    }
}

pub fn main(_: Vec<String>) -> isize {
    // TODO: Doesn't work??
    let _guard = Guard;

    let mut output = String::new();
    macro_rules! print {
        ($($arg:tt)*) => {
            write!(output, $($arg)*).unwrap();
        }
    }
    macro_rules! println {
        ($($arg:tt)*) => {
            writeln!(output, $($arg)*).unwrap();
        }
    }

    let current_task = task::get_my_current_task().expect("couldn't get current task");
    let directory = current_task.namespace.dir();

    let mut test_paths = Vec::new();

    for file in directory.get_files_starting_with("test_") {
        let name = file.lock().get_name();
        log::info!("NAME: {name:#?}");
        if name.starts_with("test_runner-") {
            continue;
        }

        test_paths.push(path::Path::new(file.lock().get_absolute_path()));
    }

    let mut failed = false;
    let mut failures = Vec::new();

    let num_tests = test_paths.len();
    println!("running {} tests", num_tests);

    for path in test_paths {
        print!("test {path} ... ");

        let exit_value = spawn::new_application_task_builder(path.clone(), None)
            .expect("failed to create task")
            .spawn()
            .expect("failed to spawn task")
            .join()
            .expect("failed to join task");

        match exit_value {
            ExitValue::Completed(exit_code) => match exit_code.downcast::<isize>().ok() {
                Some(exit_code) => match *exit_code {
                    0 => {
                        println!("ok");
                    }
                    exit_code => {
                        println!("FAILED");
                        failures.push(path);
                        // println!();
                        // println!("test exited with non-zero exit code: {exit_code}");
                        failed = true;
                    }
                },
                None => {
                    println!("FAILED");
                    // println!();
                    // println!("failed to downcast test exit value");
                    failures.push(path);
                    failed = true;
                }
            },
            ExitValue::Killed(kill_reason) => {
                println!("FAILED");
                // println!();
                // println!("test was killed: {kill_reason:?}");
                failures.push(path);
                failed = true;
            }
        }
    }

    println!();
    println!("failures:");
    for path in failures.iter() {
        println!("    {path}");
    }

    let num_failed = failures.len();
    let num_passed = num_tests - num_failed;

    println!();
    print!("test result: ");
    if failed {
        print!("FAILED");
    } else {
        print!("ok");
    }
    println!(". {num_passed} passed; {num_failed} failed");

    let serial_port = serial_port::get_serial_port(serial_port::SerialPortAddress::COM1)
        .expect("couldn't get serial port");
    serial_port
        .lock()
        .write_str(&output)
        .expect("failed to write to serial port");

    if failed {
        exit_qemu(ExitCode::Failed);
    } else {
        exit_qemu(ExitCode::Success);
    }
}

pub enum ExitCode {
    // TODO: 33
    Success = 0x10,
    Failed = 0x11,
}

pub fn exit_qemu(exit_code: ExitCode) -> ! {
    let mut port = x86_64::instructions::port::Port::new(0xf4);
    unsafe { port.write(exit_code as u32) };
    loop {}
}
