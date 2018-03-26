use alloc::vec::Vec;
use alloc::string::String;

use super::AmlError;
use super::parser::{ AmlParseType, ParseResult, AmlExecutionContext, ExecutionState };
use super::namespace::{ AmlValue, ObjectReference };

use super::type2opcode::{parse_def_buffer, parse_def_package, parse_def_var_package};
use super::termlist::parse_term_arg;
use super::namestring::parse_super_name;

pub fn parse_data_obj(data: &[u8],
                      ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_selector! {
        data, ctx,
        parse_computational_data,
        parse_def_package,
        parse_def_var_package
    };

    Err(AmlError::AmlInvalidOpCode)
}

pub fn parse_data_ref_obj(data: &[u8],
                          ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_selector! {
        data, ctx,
        parse_data_obj,
        parse_term_arg
    };

    match parse_super_name(data, ctx) {
        Ok(res) => match res.val {
            AmlValue::String(s) => Ok(AmlParseType {
                val: AmlValue::ObjectReference(ObjectReference::Object(s)),
                len: res.len
            }),
            _ => Ok(res)
        },
        Err(e) => Err(e)
    }
}

pub fn parse_arg_obj(data: &[u8],
                     ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    match data[0] {
        0x68 ... 0x6E => Ok(AmlParseType {
            val: AmlValue::ObjectReference(ObjectReference::ArgObj(data[0] - 0x68)),
            len: 1 as usize
        }),
        _ => Err(AmlError::AmlInvalidOpCode)
    }
}

pub fn parse_local_obj(data: &[u8],
                       ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    match data[0] {
        0x68 ... 0x6E => Ok(AmlParseType {
            val: AmlValue::ObjectReference(ObjectReference::LocalObj(data[0] - 0x60)),
            len: 1 as usize
        }),
        _ => Err(AmlError::AmlInvalidOpCode)
    }
}

fn parse_computational_data(data: &[u8],
                            ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    match data[0] {
        0x0A => Ok(AmlParseType {
            val: AmlValue::Integer(data[1] as u64),
            len: 2 as usize
        }),
        0x0B => {
            let res = (data[1] as u16) +
                ((data[2] as u16) << 8);

            Ok(AmlParseType {
                val: AmlValue::Integer(res as u64),
                len: 3 as usize
            })
        },
        0x0C => {
            let res = (data[1] as u32) +
                ((data[2] as u32) << 8) +
                ((data[3] as u32) << 16) +
                ((data[4] as u32) << 24);

            Ok(AmlParseType {
                val: AmlValue::Integer(res as u64),
                len: 5 as usize
            })
        },
        0x0D => {
            let mut cur_ptr: usize = 1;
            let mut cur_string: Vec<u8> = vec!();

            while data[cur_ptr] != 0x00 {
                cur_string.push(data[cur_ptr]);
                cur_ptr += 1;
            }

            match String::from_utf8(cur_string) {
                Ok(s) => Ok(AmlParseType {
                    val: AmlValue::String(s.clone()),
                    len: s.clone().len() + 2
                }),
                Err(_) => Err(AmlError::AmlParseError("String data - invalid string"))
            }
        },
        0x0E => {
            let res = (data[1] as u64) +
                ((data[2] as u64) << 8) +
                ((data[3] as u64) << 16) +
                ((data[4] as u64) << 24) +
                ((data[5] as u64) << 32) +
                ((data[6] as u64) << 40) +
                ((data[7] as u64) << 48) +
                ((data[8] as u64) << 56);

            Ok(AmlParseType {
                val: AmlValue::Integer(res as u64),
                len: 9 as usize
            })
        },
        0x00 => Ok(AmlParseType {
            val: AmlValue::IntegerConstant(0 as u64),
            len: 1 as usize
        }),
        0x01 => Ok(AmlParseType {
            val: AmlValue::IntegerConstant(1 as u64),
            len: 1 as usize
        }),
        0x5B => if data[1] == 0x30 {
            Ok(AmlParseType {
                val: AmlValue::IntegerConstant(2017_0630 as u64),
                len: 2 as usize
            })
        } else {
            Err(AmlError::AmlInvalidOpCode)
        },
        0xFF => Ok(AmlParseType {
            val: AmlValue::IntegerConstant(0xFFFF_FFFF_FFFF_FFFF),
            len: 1 as usize
        }),
        _ => parse_def_buffer(data, ctx)
    }
}
