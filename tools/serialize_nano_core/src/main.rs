//! Tool that creates a serialized representation of the symbols in the `nano_core` binary.

mod parse;

use crate_metadata_serde::SerializedCrate;
use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = &std::env::args().nth(1).expect("no path provided");
    let symbol_file = std::fs::read_to_string(path)?;
    let crate_items = parse::parse_nano_core_symbol_file(symbol_file)?;

    let serialized_crate = SerializedCrate {
        crate_name: "nano_core".to_string(),
        sections: crate_items.sections,
        global_sections: crate_items.global_sections,
        tls_sections: crate_items.tls_sections,
        cls_sections: crate_items.cls_sections,
        data_sections: crate_items.data_sections,
        init_symbols: crate_items.init_symbols,
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
