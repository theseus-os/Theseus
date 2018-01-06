//! # AML
//! Code to parse and execute AML tables

use alloc::string::String;
use alloc::vec::Vec;
use core::str::FromStr;

use super::sdt::Sdt;

#[macro_use]
mod parsermacros;

mod namespace;
mod termlist;
mod namespacemodifier;
mod pkglength;
mod namestring;
mod namedobj;
mod dataobj;
mod type1opcode;
mod type2opcode;
mod parser;

use self::parser::AmlExecutionContext;
use self::termlist::parse_term_list;
pub use self::namespace::AmlValue;

#[derive(Debug)]
pub enum AmlError {
    AmlParseError(&'static str),
    AmlInvalidOpCode,
    AmlValueError,
    AmlDeferredLoad,
    AmlFatalError(u8, u16, AmlValue),
    AmlHardFatal
}

pub fn parse_aml_table(sdt: &Sdt) -> Result<Vec<String>, AmlError> {
    parse_aml_with_scope(sdt, String::from_str("\\").unwrap())
}

pub fn parse_aml_with_scope(sdt: &Sdt, scope: String) -> Result<Vec<String>, AmlError> {
    let data = sdt.data();
    let mut ctx = AmlExecutionContext::new(scope);

    parse_term_list(data, &mut ctx)?;

    Ok(ctx.namespace_delta)
}

pub fn is_aml_table(sdt: &Sdt) -> bool {
    if &sdt.signature == b"DSDT" || &sdt.signature == b"SSDT" {
        true
    } else {
        false
    }
}
