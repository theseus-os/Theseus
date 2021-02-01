cfg_if::cfg_if!{

if #[cfg(target_arch="arm")] {
    pub mod memory;
}

}
