use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use super::{AmlError, parse_aml_with_scope};
use super::parser::{AmlParseType, ParseResult, AmlExecutionContext, ExecutionState};
use super::namespace::{AmlValue, ObjectReference};
use super::pkglength::parse_pkg_length;
use super::termlist::{parse_term_arg, parse_method_invocation};
use super::namestring::{parse_super_name, parse_target, parse_name_string, parse_simple_name};
use super::dataobj::parse_data_ref_obj;

use time::monotonic;
use acpi::SDT_POINTERS;

#[derive(Debug, Clone)]
pub enum MatchOpcode {
    MTR,
    MEQ,
    MLE,
    MLT,
    MGE,
    MGT
}

pub fn parse_type2_opcode(data: &[u8],
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
        parse_def_increment,
        parse_def_acquire,
        parse_def_wait,
        parse_def_land,
        parse_def_lequal,
        parse_def_lgreater,
        parse_def_lless,
        parse_def_lnot,
        parse_def_lor,
        parse_def_size_of,
        parse_def_store,
        parse_def_subtract,
        parse_def_to_buffer,
        parse_def_to_hex_string,
        parse_def_to_bcd,
        parse_def_to_decimal_string,
        parse_def_to_integer,
        parse_def_to_string,
        parse_def_add,
        parse_def_xor,
        parse_def_shift_left,
        parse_def_shift_right,
        parse_def_mod,
        parse_def_and,
        parse_def_or,
        parse_def_concat_res,
        parse_def_concat,
        parse_def_cond_ref_of,
        parse_def_copy_object,
        parse_def_decrement,
        parse_def_divide,
        parse_def_find_set_left_bit,
        parse_def_find_set_right_bit,
        parse_def_from_bcd,
        parse_def_load_table,
        parse_def_match,
        parse_def_mid,
        parse_def_multiply,
        parse_def_nand,
        parse_def_nor,
        parse_def_not,
        parse_def_timer,
        parse_def_buffer,
        parse_def_package,
        parse_def_var_package,
        parse_def_object_type,
        parse_def_deref_of,
        parse_def_ref_of,
        parse_def_index,
        parse_method_invocation
    };

    Err(AmlError::AmlInvalidOpCode)
}

pub fn parse_type6_opcode(data: &[u8],
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
        parse_def_deref_of,
        parse_def_ref_of,
        parse_def_index,
        parse_method_invocation
    };

    Err(AmlError::AmlInvalidOpCode)
}

pub fn parse_def_object_type(data: &[u8],
                             ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x8E);
    parser_selector! {
        data, ctx,
        parse_super_name,
        parse_def_ref_of,
        parse_def_deref_of,
        parse_def_index
    }

    Err(AmlError::AmlInvalidOpCode)
}

pub fn parse_def_package(data: &[u8],
                         ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    // TODO: Handle deferred loads in here
    parser_opcode!(data, 0x12);

    let (pkg_length, pkg_length_len) = parse_pkg_length(&data[1..])?;
    let numelements = data[1 + pkg_length_len] as usize;
    let mut elements = parse_package_elements_list(&data[2 + pkg_length_len .. 1 + pkg_length], ctx)?.val.get_as_package()?;

    if elements.len() > numelements {
        elements = elements[0 .. numelements].to_vec();
    } else if numelements > elements.len() {
        for _ in 0..numelements - elements.len() {
            elements.push(AmlValue::Uninitialized);
        }
    }

    Ok(AmlParseType {
        val: AmlValue::Package(elements),
        len: 1 + pkg_length
    })
}

pub fn parse_def_var_package(data: &[u8],
                             ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    // TODO: Handle deferred loads in here
    parser_opcode!(data, 0x13);

    let (pkg_length, pkg_length_len) = parse_pkg_length(&data[1..])?;
    let num_elements = parse_term_arg(&data[1 + pkg_length_len .. 1 + pkg_length], ctx)?;
    let mut elements = parse_package_elements_list(&data[1 + pkg_length_len + num_elements.len ..
                                                         1 + pkg_length], ctx)?.val.get_as_package()?;

    let numelements = num_elements.val.get_as_integer()? as usize;

    if elements.len() > numelements {
        elements = elements[0 .. numelements].to_vec();
    } else if numelements > elements.len() {
        for _ in 0..numelements - elements.len() {
            elements.push(AmlValue::Uninitialized);
        }
    }

    Ok(AmlParseType {
        val: AmlValue::Package(elements),
        len: 1 + pkg_length
    })
}

fn parse_package_elements_list(data: &[u8],
                               ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    let mut current_offset: usize = 0;
    let mut elements: Vec<AmlValue> = vec!();

    while current_offset < data.len() {
        let dro = if let Ok(e) = parse_data_ref_obj(&data[current_offset..], ctx) {
            e
        } else {
            let d = parse_name_string(&data[current_offset..], ctx)?;
            AmlParseType {
                val: AmlValue::ObjectReference(ObjectReference::Object(d.val.get_as_string()?)),
                len: d.len
            }
        };

        elements.push(dro.val);
        current_offset += dro.len;
    }

    Ok(AmlParseType {
        val: AmlValue::Package(elements),
        len: data.len()
    })
}

pub fn parse_def_buffer(data: &[u8],
                        ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x11);

    let (pkg_length, pkg_length_len) = parse_pkg_length(&data[1..])?;
    let buffer_size = parse_term_arg(&data[1 + pkg_length_len..], ctx)?;
    let mut byte_list = data[1 + pkg_length_len + buffer_size.len .. 1 + pkg_length].to_vec().clone();

    byte_list.truncate(buffer_size.val.get_as_integer()? as usize);

    Ok(AmlParseType {
        val: AmlValue::Buffer(byte_list),
        len: 1 + pkg_length
    })
}

fn parse_def_ref_of(data: &[u8],
                    ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x71);

    let obj = parse_super_name(&data[1..], ctx)?;
    let res = match obj.val {
        AmlValue::String(ref s) => {
            match ctx.get(AmlValue::String(s.clone()))? {
                AmlValue::None => return Err(AmlError::AmlValueError),
                _ => ObjectReference::Object(s.clone())
            }
        },
        AmlValue::ObjectReference(ref o) => o.clone(),
        _ => return Err(AmlError::AmlValueError)
    };

    Ok(AmlParseType {
        val: AmlValue::ObjectReference(res),
        len: 1 + obj.len
    })
}

fn parse_def_deref_of(data: &[u8],
                      ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x83);

    let obj = parse_term_arg(&data[1..], ctx)?;
    let res = ctx.get(obj.val)?;

    match res {
        AmlValue::None => Err(AmlError::AmlValueError),
        _ => Ok(AmlParseType {
            val: res,
            len: 1 + obj.len
        })
    }
}

fn parse_def_acquire(data: &[u8],
                     ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x23);

    let obj = parse_super_name(&data[1..], ctx)?;
    let timeout = (data[2 + obj.len] as u16) + ((data[3 + obj.len] as u16) << 8);

    let (seconds, nanoseconds) = monotonic();
    let starting_time_ns = nanoseconds + (seconds * 1_000_000_000);

    loop {
        match ctx.acquire_mutex(obj.val.clone()) {
            Err(e) => return Err(e),
            Ok(b) => if b {
                return Ok(AmlParseType {
                    val: AmlValue::Integer(0),
                    len: 4 + obj.len
                });
            } else if timeout == 0xFFFF {
                // TODO: Brief sleep here
            } else {
                let (seconds, nanoseconds) = monotonic();
                let current_time_ns = nanoseconds + (seconds * 1_000_000_000);

                if current_time_ns - starting_time_ns > timeout as u64 * 1_000_000 {
                    return Ok(AmlParseType {
                        val: AmlValue::Integer(1),
                        len: 4 + obj.len
                    });
                }
            }
        }
    }
}

fn parse_def_increment(data: &[u8],
                       ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x75);

    let obj = parse_super_name(&data[1..], ctx)?;

    let _namespace = ctx.prelock();
    let value = AmlValue::Integer(ctx.get(obj.val.clone())?.get_as_integer()? + 1);
    let _ = ctx.modify(obj.val, value.clone());

    Ok(AmlParseType {
        val: value,
        len: 1 + obj.len
    })
}

fn parse_def_index(data: &[u8],
                   ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x88);

    let obj = parse_term_arg(&data[1..], ctx)?;
    let idx = parse_term_arg(&data[1 + obj.len..], ctx)?;
    let target = parse_target(&data[1 + obj.len + idx.len..], ctx)?;

    let reference = AmlValue::ObjectReference(ObjectReference::Index(Box::new(obj.val), Box::new(idx.val)));
    let _ = ctx.modify(target.val, reference.clone());

    Ok(AmlParseType {
        val: reference,
        len: 1 + obj.len + idx.len + target.len
    })
}

fn parse_def_land(data: &[u8],
                  ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x90);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;

    let result = if lhs.val.get_as_integer()? > 0 && rhs.val.get_as_integer()? > 0 { 1 } else { 0 };

    Ok(AmlParseType {
        val: AmlValue::IntegerConstant(result),
        len: 1 + lhs.len + rhs.len
    })
}

fn parse_def_lequal(data: &[u8],
                    ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x93);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;

    let result = if lhs.val.get_as_integer()? == rhs.val.get_as_integer()? { 1 } else { 0 };

    Ok(AmlParseType {
        val: AmlValue::IntegerConstant(result),
        len: 1 + lhs.len + rhs.len
    })
}

fn parse_def_lgreater(data: &[u8],
                      ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x94);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;

    let result = if lhs.val.get_as_integer()? > rhs.val.get_as_integer()? { 1 } else { 0 };

    Ok(AmlParseType {
        val: AmlValue::IntegerConstant(result),
        len: 1 + lhs.len + rhs.len
    })
}

fn parse_def_lless(data: &[u8],
                   ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x95);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;

    let result = if lhs.val.get_as_integer()? < rhs.val.get_as_integer()? { 1 } else { 0 };

    Ok(AmlParseType {
        val: AmlValue::IntegerConstant(result),
        len: 1 + lhs.len + rhs.len
    })
}

fn parse_def_lnot(data: &[u8],
                  ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x92);

    let operand = parse_term_arg(&data[1..], ctx)?;
    let result = if operand.val.get_as_integer()? == 0 { 1 } else { 0 };

    Ok(AmlParseType {
        val: AmlValue::IntegerConstant(result),
        len: 1 + operand.len
    })
}

fn parse_def_lor(data: &[u8],
                 ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x91);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;

    let result = if lhs.val.get_as_integer()? > 0 || rhs.val.get_as_integer()? > 0 { 1 } else { 0 };

    Ok(AmlParseType {
        val: AmlValue::IntegerConstant(result),
        len: 1 + lhs.len + rhs.len
    })
}

fn parse_def_to_hex_string(data: &[u8],
                           ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x98);

    let operand = parse_term_arg(&data[2..], ctx)?;
    let target = parse_target(&data[2 + operand.len..], ctx)?;

    let res = match operand.val {
        AmlValue::Integer(_) => {
            let result: String = format!("{:X}", operand.val.get_as_integer()?);
            AmlValue::String(result)
        },
        AmlValue::String(s) => AmlValue::String(s),
        AmlValue::Buffer(_) => {
            let mut string: String = String::new();

            for b in operand.val.get_as_buffer()? {
                string.push_str(&format!("{:X}", b));
            }

            AmlValue::String(string)
        },
        _ => return Err(AmlError::AmlValueError)
    };

    let _ = ctx.modify(target.val, res.clone());

    Ok(AmlParseType {
        val: res,
        len: 1 + operand.len + target.len
    })
}

fn parse_def_to_buffer(data: &[u8],
                       ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x96);

    let operand = parse_term_arg(&data[2..], ctx)?;
    let target = parse_target(&data[2 + operand.len..], ctx)?;

    let res = AmlValue::Buffer(operand.val.get_as_buffer()?);
    let _ = ctx.modify(target.val, res.clone());

    Ok(AmlParseType {
        val: res,
        len: 1 + operand.len + target.len
    })
}

fn parse_def_to_bcd(data: &[u8],
                    ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x29);

    let operand = parse_term_arg(&data[2..], ctx)?;
    let target = parse_target(&data[2 + operand.len..], ctx)?;

    let mut i = operand.val.get_as_integer()?;
    let mut result = 0;

    while i != 0 {
        result <<= 4;
        result += i % 10;
        i /= 10;
    }

    let result = AmlValue::Integer(result);
    let _ = ctx.modify(target.val, result.clone());

    Ok(AmlParseType {
        val: result,
        len: 1 + operand.len + target.len
    })
}

fn parse_def_to_decimal_string(data: &[u8],
                               ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x97);

    let operand = parse_term_arg(&data[2..], ctx)?;
    let target = parse_target(&data[2 + operand.len..], ctx)?;
    let res = match operand.val {
        AmlValue::Integer(_) => {
            let result: String = format!("{}", operand.val.get_as_integer()?);
            AmlValue::String(result)
        },
        AmlValue::String(s) => AmlValue::String(s),
        AmlValue::Buffer(_) => {
            let mut string: String = String::new();

            for b in operand.val.get_as_buffer()? {
                string.push_str(&format!("{}", b));
            }

            AmlValue::String(string)
        },
        _ => return Err(AmlError::AmlValueError)
    };

    let _ = ctx.modify(target.val, res.clone());

    Ok(AmlParseType {
        val: res,
        len: 1 + operand.len + target.len
    })
}

fn parse_def_to_integer(data: &[u8],
                        ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x99);

    let operand = parse_term_arg(&data[2..], ctx)?;
    let target = parse_target(&data[2 + operand.len..], ctx)?;

    let res = AmlValue::Integer(operand.val.get_as_integer()?);

    let _ = ctx.modify(target.val, res.clone());

    Ok(AmlParseType {
        val: res,
        len: 1 + operand.len + target.len
    })
}

fn parse_def_to_string(data: &[u8],
                       ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x9C);

    let operand = parse_term_arg(&data[1..], ctx)?;
    let length = parse_term_arg(&data[1 + operand.len..], ctx)?;
    let target = parse_target(&data[1 + operand.len + length.len..], ctx)?;

    let buf = operand.val.get_as_buffer()?;
    let mut string = match String::from_utf8(buf) {
        Ok(s) => s,
        Err(_) => return Err(AmlError::AmlValueError)
    };

    string.truncate(length.val.get_as_integer()? as usize);
    let res = AmlValue::String(string);

    let _ = ctx.modify(target.val, res.clone());

    Ok(AmlParseType {
        val: res,
        len: 1 + operand.len + length.len + target.len
    })
}

fn parse_def_subtract(data: &[u8],
                      ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x74);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;
    let target = parse_target(&data[1 + lhs.len + rhs.len..], ctx)?;

    let result = AmlValue::Integer(lhs.val.get_as_integer()? - rhs.val.get_as_integer()?);

    let _ = ctx.modify(target.val, result.clone());

    Ok(AmlParseType {
        val: result,
        len: 1 + lhs.len + rhs.len + target.len
    })
}

fn parse_def_size_of(data: &[u8],
                     ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x87);

    let name = parse_super_name(&data[1..], ctx)?;
    let obj = ctx.get(name.val)?;

    let res = match obj {
        AmlValue::Buffer(ref v) => v.len(),
        AmlValue::String(ref s) => s.len(),
        AmlValue::Package(ref p) => p.len(),
        _ => return Err(AmlError::AmlValueError)
    };

    Ok(AmlParseType {
        val: AmlValue::Integer(res as u64),
        len: 1 + name.len
    })
}

fn parse_def_store(data: &[u8],
                   ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x70);

    let operand = parse_term_arg(&data[1..], ctx)?;
    let target = parse_super_name(&data[1 + operand.len..], ctx)?;

    let _ = ctx.modify(target.val.clone(), operand.val);

    Ok(AmlParseType {
        val: target.val,
        len: 1 + operand.len + target.len
    })
}

fn parse_def_or(data: &[u8],
                ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x7D);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;
    let target = parse_target(&data[1 + lhs.len + rhs.len..], ctx)?;

    let result = AmlValue::Integer(lhs.val.get_as_integer()? | rhs.val.get_as_integer()?);

    let _ = ctx.modify(target.val, result.clone());

    Ok(AmlParseType {
        val: result,
        len: 1 + lhs.len + rhs.len + target.len
    })
}

fn parse_def_shift_left(data: &[u8],
                        ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x79);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;
    let target = parse_target(&data[1 + lhs.len + rhs.len..], ctx)?;

    let result = AmlValue::Integer(lhs.val.get_as_integer()? >> rhs.val.get_as_integer()?);

    let _ = ctx.modify(target.val, result.clone());

    Ok(AmlParseType {
        val: result,
        len: 1 + lhs.len + rhs.len + target.len
    })
}

fn parse_def_shift_right(data: &[u8],
                         ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x7A);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;
    let target = parse_target(&data[1 + lhs.len + rhs.len..], ctx)?;

    let result = AmlValue::Integer(lhs.val.get_as_integer()? << rhs.val.get_as_integer()?);

    let _ = ctx.modify(target.val, result.clone());

    Ok(AmlParseType {
        val: result,
        len: 1 + lhs.len + rhs.len + target.len
    })
}

fn parse_def_add(data: &[u8],
                 ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x72);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;
    let target = parse_target(&data[1 + lhs.len + rhs.len..], ctx)?;

    let result = AmlValue::Integer(lhs.val.get_as_integer()? + rhs.val.get_as_integer()?);

    let _ = ctx.modify(target.val, result.clone());

    Ok(AmlParseType {
        val: result,
        len: 1 + lhs.len + rhs.len + target.len
    })
}

fn parse_def_and(data: &[u8],
                 ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x7B);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;
    let target = parse_target(&data[1 + lhs.len + rhs.len..], ctx)?;

    let result = AmlValue::Integer(lhs.val.get_as_integer()? & rhs.val.get_as_integer()?);

    let _ = ctx.modify(target.val, result.clone());

    Ok(AmlParseType {
        val: result,
        len: 1 + lhs.len + rhs.len + target.len
    })
}

fn parse_def_xor(data: &[u8],
                 ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x7F);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;
    let target = parse_target(&data[1 + lhs.len + rhs.len..], ctx)?;

    let result = AmlValue::Integer(lhs.val.get_as_integer()? ^ rhs.val.get_as_integer()?);

    let _ = ctx.modify(target.val, result.clone());

    Ok(AmlParseType {
        val: result,
        len: 1 + lhs.len + rhs.len + target.len
    })
}

fn parse_def_concat_res(data: &[u8],
                        ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x84);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;
    let target = parse_target(&data[1 + lhs.len + rhs.len..], ctx)?;

    let mut buf1 = lhs.val.get_as_buffer()?.clone();
    let mut buf2 = rhs.val.get_as_buffer()?.clone();

    if buf1.len() == 1 || buf2.len() == 1 {
        return Err(AmlError::AmlValueError);
    }

    if buf1.len() >= 2 && buf1[buf1.len() - 2] == 0x79 {
        buf1 = buf1[0..buf1.len() - 2].to_vec();
    }

    if buf2.len() >= 2 && buf2[buf2.len() - 2] == 0x79 {
        buf2 = buf2[0..buf2.len() - 2].to_vec();
    }

    buf1.append(&mut buf2);
    buf1.push(0x79);

    let mut checksum: u8 = 0;
    let loopbuf = buf1.clone();
    for b in loopbuf {
        checksum += b;
    }

    checksum = (!checksum) + 1;
    buf1.push(checksum);

    let res = AmlValue::Buffer(buf1);
    ctx.modify(target.val, res.clone())?;

    Ok(AmlParseType {
        val: res,
        len: 1 + lhs.len + rhs.len + target.len
    })
}

fn parse_def_wait(data: &[u8],
                  ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x25);

    let obj = parse_super_name(&data[2..], ctx)?;
    let timeout_obj = parse_term_arg(&data[2 + obj.len..], ctx)?;

    let timeout = timeout_obj.val.get_as_integer()?;

    let (seconds, nanoseconds) = monotonic();
    let starting_time_ns = nanoseconds + (seconds * 1_000_000_000);

    loop {
        match ctx.wait_for_event(obj.val.clone()) {
            Err(e) => return Err(e),
            Ok(b) => if b {
                return Ok(AmlParseType {
                    val: AmlValue::Integer(0),
                    len: 2 + obj.len + timeout_obj.len
                })
            } else if timeout >= 0xFFFF {
                // TODO: Brief sleep here
            } else {
                let (seconds, nanoseconds) = monotonic();
                let current_time_ns = nanoseconds + (seconds * 1_000_000_000);

                if current_time_ns - starting_time_ns > timeout as u64 * 1_000_000 {
                    return Ok(AmlParseType {
                        val: AmlValue::Integer(1),
                        len: 2 + obj.len + timeout_obj.len
                    });
                }
            }
        }
    }
}

fn parse_def_cond_ref_of(data: &[u8],
                         ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x12);

    let obj = parse_super_name(&data[2..], ctx)?;
    let target = parse_target(&data[2 + obj.len..], ctx)?;

    let res = match obj.val {
        AmlValue::String(ref s) => {
            match ctx.get(AmlValue::String(s.clone()))? {
                AmlValue::None => return Ok(AmlParseType {
                    val: AmlValue::Integer(0),
                    len: 1 + obj.len + target.len
                }),
                _ => ObjectReference::Object(s.clone())
            }
        },
        AmlValue::ObjectReference(ref o) => o.clone(),
        _ => return Err(AmlError::AmlValueError)
    };

    let _ = ctx.modify(target.val, AmlValue::ObjectReference(res));

    Ok(AmlParseType {
        val: AmlValue::Integer(1),
        len: 1 + obj.len + target.len
    })
}

fn parse_def_copy_object(data: &[u8],
                         ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    // TODO: Compute the result
    // TODO: Store the result
    parser_opcode!(data, 0x9D);

    let source = parse_term_arg(&data[1..], ctx)?;
    let destination = parse_simple_name(&data[1 + source.len..], ctx)?;

    ctx.copy(destination.val, source.val.clone())?;

    Ok(AmlParseType {
        val: source.val,
        len: 1 + source.len + destination.len
    })
}

fn parse_def_concat(data: &[u8],
                    ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x73);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;
    let target = parse_target(&data[1 + lhs.len + rhs.len..], ctx)?;

    let result = match lhs.val {
        AmlValue::Integer(_i) => {
            let j = AmlValue::Integer(rhs.val.get_as_integer()?);

            let mut first = lhs.val.get_as_buffer()?.clone();
            let mut second = j.get_as_buffer()?.clone();

            first.append(&mut second);

            AmlValue::Buffer(first)
        },
        AmlValue::String(s) => {
            let t = if let Ok(t) = rhs.val.get_as_string() {
                t
            } else {
                rhs.val.get_type_string()
            };

            AmlValue::String(format!("{}{}", s, t))
        },
        AmlValue::Buffer(b) => {
            let mut b = b.clone();
            let mut c = if let Ok(c) = rhs.val.get_as_buffer() {
                c.clone()
            } else {
                AmlValue::String(rhs.val.get_type_string()).get_as_buffer()?.clone()
            };

            b.append(&mut c);

            AmlValue::Buffer(b)
        },
        _ => {
            let first = lhs.val.get_type_string();
            let second = if let Ok(second) = rhs.val.get_as_string() {
                second
            } else {
                rhs.val.get_type_string()
            };

            AmlValue::String(format!("{}{}", first, second))
        }
    };

    ctx.modify(target.val, result.clone())?;

    Ok(AmlParseType {
        val: result,
        len: 1 + lhs.len + rhs.len + target.len
    })
}

fn parse_def_decrement(data: &[u8],
                       ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x76);

    let obj = parse_super_name(&data[1..], ctx)?;

    let _namespace = ctx.prelock();
    let value = AmlValue::Integer(ctx.get(obj.val.clone())?.get_as_integer()? - 1);
    let _ = ctx.modify(obj.val, value.clone());

    Ok(AmlParseType {
        val: value,
        len: 1 + obj.len
    })
}

fn parse_def_divide(data: &[u8],
                    ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x78);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;
    let target_remainder = parse_target(&data[1 + lhs.len + rhs.len..], ctx)?;
    let target_quotient = parse_target(&data[1 + lhs.len + rhs.len + target_remainder.len..], ctx)?;

    let numerator = lhs.val.get_as_integer()?;
    let denominator = rhs.val.get_as_integer()?;

    let remainder = numerator % denominator;
    let quotient = (numerator - remainder) / denominator;

    let _ = ctx.modify(target_remainder.val, AmlValue::Integer(remainder));
    let _ = ctx.modify(target_quotient.val, AmlValue::Integer(quotient));

    Ok(AmlParseType {
        val: AmlValue::Integer(quotient),
        len: 1 + lhs.len + rhs.len + target_remainder.len + target_quotient.len
    })
}

fn parse_def_find_set_left_bit(data: &[u8],
                               ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x81);

    let operand = parse_term_arg(&data[2..], ctx)?;
    let target = parse_target(&data[2 + operand.len..], ctx)?;

    let mut first_bit = 32;
    let mut test = operand.val.get_as_integer()?;

    while first_bit > 0{
        if test & 0x8000_0000_0000_0000 > 0 {
            break;
        }

        test <<= 1;
        first_bit -= 1;
    }

    let result = AmlValue::Integer(first_bit);
    let _ = ctx.modify(target.val, result.clone());

    Ok(AmlParseType {
        val: result,
        len: 1 + operand.len + target.len
    })
}

fn parse_def_find_set_right_bit(data: &[u8],
                                ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x82);

    let operand = parse_term_arg(&data[2..], ctx)?;
    let target = parse_target(&data[2 + operand.len..], ctx)?;

    let mut first_bit = 1;
    let mut test = operand.val.get_as_integer()?;

    while first_bit <= 32 {
        if test & 1 > 0 {
            break;
        }

        test >>= 1;
        first_bit += 1;
    }

    if first_bit == 33 {
        first_bit = 0;
    }

    let result = AmlValue::Integer(first_bit);
    let _ = ctx.modify(target.val, result.clone());

    Ok(AmlParseType {
        val: result,
        len: 1 + operand.len + target.len
    })
}

fn parse_def_load_table(data: &[u8],
                        ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    // TODO: Clean up
    parser_opcode_extended!(data, 0x1F);

    let signature = parse_term_arg(&data[2..], ctx)?;
    let oem_id = parse_term_arg(&data[2 + signature.len..], ctx)?;
    let oem_table_id = parse_term_arg(&data[2 + signature.len + oem_id.len..], ctx)?;
    let root_path = parse_term_arg(&data[2 + signature.len + oem_id.len + oem_table_id.len..], ctx)?;
    let parameter_path = parse_term_arg(&data[2 + signature.len + oem_id.len + oem_table_id.len + root_path.len..], ctx)?;
    let parameter_data = parse_term_arg(&data[2 + signature.len + oem_id.len + oem_table_id.len + root_path.len + parameter_path.len..], ctx)?;

    if let Some(ref ptrs) = *(SDT_POINTERS.read()) {
        let sig_str = unsafe {
            let sig = *(signature.val.get_as_string()?.as_bytes().as_ptr() as *const [u8; 4]);
            String::from_utf8(sig.to_vec()).expect("Error converting signature to string")
        };
        let oem_str = unsafe {
            *(oem_id.val.get_as_string()?.as_bytes().as_ptr() as *const [u8; 6])
        };
        let oem_table_str = unsafe {
            *(oem_table_id.val.get_as_string()?.as_bytes().as_ptr() as *const [u8; 8])
        };

        let sdt_signature = (sig_str, oem_str, oem_table_str);

        let sdt = ptrs.get(&sdt_signature);

        if let Some(sdt) = sdt {
            let hdl = parse_aml_with_scope(sdt, root_path.val.get_as_string()?)?;
            let _ = ctx.modify(parameter_path.val, parameter_data.val);

            return Ok(AmlParseType {
                val: AmlValue::DDBHandle((hdl, sdt_signature)),
                len: 2 + signature.len + oem_id.len + oem_table_id.len + root_path.len + parameter_path.len + parameter_data.len
            });
        }
    }

    Ok(AmlParseType {
        val: AmlValue::IntegerConstant(0),
        len: 2 + signature.len + oem_id.len + oem_table_id.len + root_path.len + parameter_path.len + parameter_data.len
    })
}

fn parse_def_match(data: &[u8],
                   ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x28);

    let search_pkg = parse_term_arg(&data[1..], ctx)?;

    let first_operation = match data[1 + search_pkg.len] {
        0 => MatchOpcode::MTR,
        1 => MatchOpcode::MEQ,
        2 => MatchOpcode::MLE,
        3 => MatchOpcode::MLT,
        4 => MatchOpcode::MGE,
        5 => MatchOpcode::MGT,
        _ => return Err(AmlError::AmlParseError("DefMatch - Invalid Opcode"))
    };
    let first_operand = parse_term_arg(&data[2 + search_pkg.len..], ctx)?;

    let second_operation = match data[2 + search_pkg.len + first_operand.len] {
        0 => MatchOpcode::MTR,
        1 => MatchOpcode::MEQ,
        2 => MatchOpcode::MLE,
        3 => MatchOpcode::MLT,
        4 => MatchOpcode::MGE,
        5 => MatchOpcode::MGT,
        _ => return Err(AmlError::AmlParseError("DefMatch - Invalid Opcode"))
    };
    let second_operand = parse_term_arg(&data[3 + search_pkg.len + first_operand.len..], ctx)?;

    let start_index = parse_term_arg(&data[3 + search_pkg.len + first_operand.len + second_operand.len..], ctx)?;

    let pkg = search_pkg.val.get_as_package()?;
    let mut idx = start_index.val.get_as_integer()? as usize;

    match first_operand.val {
        AmlValue::Integer(i) => {
            let j = second_operand.val.get_as_integer()?;

            while idx < pkg.len() {
                let val = if let Ok(v) = pkg[idx].get_as_integer() { v } else { idx += 1; continue; };
                idx += 1;

                match first_operation {
                    MatchOpcode::MTR => (),
                    MatchOpcode::MEQ => if val != i { continue },
                    MatchOpcode::MLE => if val > i { continue },
                    MatchOpcode::MLT => if val >= i { continue },
                    MatchOpcode::MGE => if val < i { continue },
                    MatchOpcode::MGT => if val <= i { continue }
                }

                match second_operation {
                    MatchOpcode::MTR => (),
                    MatchOpcode::MEQ => if val != j { continue },
                    MatchOpcode::MLE => if val > j { continue },
                    MatchOpcode::MLT => if val >= j { continue },
                    MatchOpcode::MGE => if val < j { continue },
                    MatchOpcode::MGT => if val <= j { continue }
                }

                return Ok(AmlParseType {
                    val: AmlValue::Integer(idx as u64),
                    len: 3 + search_pkg.len + first_operand.len + second_operand.len + start_index.len
                })
            }
        },
        AmlValue::String(i) => {
            let j = second_operand.val.get_as_string()?;

            while idx < pkg.len() {
                let val = if let Ok(v) = pkg[idx].get_as_string() { v } else { idx += 1; continue; };
                idx += 1;

                match first_operation {
                    MatchOpcode::MTR => (),
                    MatchOpcode::MEQ => if val != i { continue },
                    MatchOpcode::MLE => if val > i { continue },
                    MatchOpcode::MLT => if val >= i { continue },
                    MatchOpcode::MGE => if val < i { continue },
                    MatchOpcode::MGT => if val <= i { continue }
                }

                match second_operation {
                    MatchOpcode::MTR => (),
                    MatchOpcode::MEQ => if val != j { continue },
                    MatchOpcode::MLE => if val > j { continue },
                    MatchOpcode::MLT => if val >= j { continue },
                    MatchOpcode::MGE => if val < j { continue },
                    MatchOpcode::MGT => if val <= j { continue }
                }

                return Ok(AmlParseType {
                    val: AmlValue::Integer(idx as u64),
                    len: 3 + search_pkg.len + first_operand.len + second_operand.len + start_index.len
                })
            }
        },
        _ => {
            let i = first_operand.val.get_as_buffer()?;
            let j = second_operand.val.get_as_buffer()?;

            while idx < pkg.len() {
                let val = if let Ok(v) = pkg[idx].get_as_buffer() { v } else { idx += 1; continue; };
                idx += 1;

                match first_operation {
                    MatchOpcode::MTR => (),
                    MatchOpcode::MEQ => if val != i { continue },
                    MatchOpcode::MLE => if val > i { continue },
                    MatchOpcode::MLT => if val >= i { continue },
                    MatchOpcode::MGE => if val < i { continue },
                    MatchOpcode::MGT => if val <= i { continue }
                }

                match second_operation {
                    MatchOpcode::MTR => (),
                    MatchOpcode::MEQ => if val != j { continue },
                    MatchOpcode::MLE => if val > j { continue },
                    MatchOpcode::MLT => if val >= j { continue },
                    MatchOpcode::MGE => if val < j { continue },
                    MatchOpcode::MGT => if val <= j { continue }
                }

                return Ok(AmlParseType {
                    val: AmlValue::Integer(idx as u64),
                    len: 3 + search_pkg.len + first_operand.len + second_operand.len + start_index.len
                })
            }
        }
    }

    Ok(AmlParseType {
        val: AmlValue::IntegerConstant(0xFFFF_FFFF_FFFF_FFFF),
        len: 3 + search_pkg.len + first_operand.len + second_operand.len + start_index.len
    })
}

fn parse_def_from_bcd(data: &[u8],
                      ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x28);

    let operand = parse_term_arg(&data[2..], ctx)?;
    let target = parse_target(&data[2 + operand.len..], ctx)?;

    let mut i = operand.val.get_as_integer()?;
    let mut result = 0;

    while i != 0 {
        if i & 0x0F > 10 {
            return Err(AmlError::AmlValueError);
        }

        result *= 10;
        result += i & 0x0F;
        i >>= 4;
    }

    let result = AmlValue::Integer(result);

    let _ = ctx.modify(target.val, result.clone());

    Ok(AmlParseType {
        val: result,
        len: 2 + operand.len + target.len
    })
}

fn parse_def_mid(data: &[u8],
                 ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x9E);

    let source = parse_term_arg(&data[1..], ctx)?;
    let index = parse_term_arg(&data[1 + source.len..], ctx)?;
    let length = parse_term_arg(&data[1 + source.len + index.len..], ctx)?;
    let target = parse_target(&data[1 + source.len + index.len + length.len..], ctx)?;

    let idx = index.val.get_as_integer()? as usize;
    let mut len = length.val.get_as_integer()? as usize;

    let result = match source.val {
        AmlValue::String(s) => {
            if idx > s.len() {
                AmlValue::String(String::new())
            } else {
                let mut res = s.clone().split_off(idx);

                if len < res.len() {
                    res.split_off(len);
                }

                AmlValue::String(res)
            }
        },
        _ => {
            // If it isn't a string already, treat it as a buffer. Must perform that check first,
            // as Mid can operate on both strings and buffers, but a string can be cast as a buffer
            // implicitly.
            // Additionally, any type that can be converted to a buffer can also be converted to a
            // string, so no information is lost
            let b = source.val.get_as_buffer()?;

            if idx > b.len() {
                AmlValue::Buffer(vec!())
            } else {
                if idx + len > b.len() {
                    len = b.len() - idx;
                }

                AmlValue::Buffer(b[idx .. idx + len].to_vec())
            }
        }
    };

    let _ = ctx.modify(target.val, result.clone());

    Ok(AmlParseType {
        val: result,
        len: 1 + source.len + index.len + length.len + target.len
    })
}

fn parse_def_mod(data: &[u8],
                 ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x85);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;
    let target = parse_target(&data[1 + lhs.len + rhs.len..], ctx)?;

    if rhs.val.get_as_integer()? == 0 {
        return Err(AmlError::AmlValueError);
    }

    let result = AmlValue::Integer(lhs.val.get_as_integer()? % rhs.val.get_as_integer()?);

    let _ = ctx.modify(target.val, result.clone());

    Ok(AmlParseType {
        val: result,
        len: 1 + lhs.len + rhs.len + target.len
    })
}

fn parse_def_multiply(data: &[u8],
                      ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    // TODO: Handle overflow
    parser_opcode!(data, 0x77);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;
    let target = parse_target(&data[1 + lhs.len + rhs.len..], ctx)?;

    let result = AmlValue::Integer(lhs.val.get_as_integer()? * rhs.val.get_as_integer()?);

    let _ = ctx.modify(target.val, result.clone());

    Ok(AmlParseType {
        val: result,
        len: 1 + lhs.len + rhs.len + target.len
    })
}

fn parse_def_nand(data: &[u8],
                  ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x7C);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;
    let target = parse_target(&data[1 + lhs.len + rhs.len..], ctx)?;

    let result = AmlValue::Integer(!(lhs.val.get_as_integer()? & rhs.val.get_as_integer()?));

    let _ = ctx.modify(target.val, result.clone());

    Ok(AmlParseType {
        val: result,
        len: 1 + lhs.len + rhs.len + target.len
    })
}

fn parse_def_nor(data: &[u8],
                 ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x7E);

    let lhs = parse_term_arg(&data[1..], ctx)?;
    let rhs = parse_term_arg(&data[1 + lhs.len..], ctx)?;
    let target = parse_target(&data[1 + lhs.len + rhs.len..], ctx)?;

    let result = AmlValue::Integer(!(lhs.val.get_as_integer()? | rhs.val.get_as_integer()?));

    let _ = ctx.modify(target.val, result.clone());

    Ok(AmlParseType {
        val: result,
        len: 1 + lhs.len + rhs.len + target.len
    })
}

fn parse_def_not(data: &[u8],
                 ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x80);

    let operand = parse_term_arg(&data[1..], ctx)?;
    let target = parse_target(&data[1 + operand.len..], ctx)?;

    let result = AmlValue::Integer(!operand.val.get_as_integer()?);

    let _ = ctx.modify(target.val, result.clone());

    Ok(AmlParseType {
        val: result,
        len: 1 + operand.len + target.len
    })
}

fn parse_def_timer(data: &[u8],
                   ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x33);

    let (seconds, nanoseconds) = monotonic();
    let monotonic_ns = nanoseconds + (seconds * 1_000_000_000);

    Ok(AmlParseType {
        val: AmlValue::Integer(monotonic_ns),
        len: 2 as usize
    })
}
