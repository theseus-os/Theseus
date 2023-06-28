fn main() {
    let fork = pty::fork::Fork::from_ptmx().unwrap();
    if let Some(master) = fork.is_parent().ok() {
        let name_ptr = master.ptsname().unwrap();
        let name = unsafe { core::ffi::CStr::from_ptr(name_ptr) }
            .to_str()
            .unwrap();
        println!("{name}");
    }
}
