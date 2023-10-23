#![no_std]

#[link(name = "libtheseus")]
extern "C" {
    #[link_name = "libtheseus::next_u64"]
    pub fn next_u64() -> u64;
}
