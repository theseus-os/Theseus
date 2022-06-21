mod parse;

use mod_mgmt::serde::SerializedCrate;
use std::{collections::BTreeSet, io::Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = &std::env::args().collect::<Vec<String>>()[1];
    let str = std::fs::read_to_string(arg)?;
    let crate_items = parse::parse_nano_core_symbol_file(str)?;

    let serialized_crate = SerializedCrate {
        crate_name: "nano_core".to_string(),
        sections: crate_items.sections,
        global_sections: crate_items.global_sections,
        tls_sections: crate_items.tls_sections,
        data_sections: crate_items.data_sections,
        init_symbols: crate_items.init_symbols,
        reexported_symbols: BTreeSet::new(),
    };

    let mut stdout = std::io::stdout();
    bincode::serde::encode_into_std_write(
        &serialized_crate,
        &mut stdout,
        bincode::config::standard(),
    )?;
    stdout.flush()?;
    Ok(())
}
