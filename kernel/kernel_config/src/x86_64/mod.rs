cfg_if::cfg_if!{

if #[cfg(target_arch="x86_64")] {
    pub mod memory;
    pub mod time;
    pub mod display;
}

}
