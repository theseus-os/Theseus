pub fn my_mod_func() {
    logger::write_fmt(format_args!("\n\nHello from my_mod_func!")).unwrap();
}
