#![feature(restricted_std)]

pub fn main(__args: Vec<String>) -> isize {
    let parent_id = std::thread::current().id();

    std::thread::spawn(move || {
        let child_id = std::thread::current().id();
        assert_ne!(parent_id, child_id);
    })
    .join()
    .unwrap();

    println!("thread test successful");

    std::env::set_var("test_std", "true");
    assert_eq!(std::env::var("test_std").unwrap(), "true");

    println!("env test successful");

    let cwd = std::env::current_dir().unwrap();
    assert_eq!(cwd, std::path::PathBuf::from("/"));

    std::env::set_current_dir("extra_files").unwrap();

    let cwd = std::env::current_dir().unwrap();
    assert_eq!(cwd, std::path::PathBuf::from("/extra_files"));

    println!("cwd test successful");

    0
}
