use super::AmlError;
use super::parser::{AmlParseType, ParseResult, AmlExecutionContext, ExecutionState};
use super::namespace::AmlValue;
use super::pkglength::parse_pkg_length;
use super::termlist::{parse_term_arg, parse_term_list};
use super::namestring::{parse_name_string, parse_super_name};

use time::monotonic;

use acpi::{Sdt, load_table, get_sdt_signature};
use super::{parse_aml_table, is_aml_table};

pub fn parse_type1_opcode(data: &[u8],
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
        parse_def_break,
        parse_def_breakpoint,
        parse_def_continue,
        parse_def_noop,
        parse_def_fatal,
        parse_def_if_else,
        parse_def_load,
        parse_def_notify,
        parse_def_release,
        parse_def_reset,
        parse_def_signal,
        parse_def_sleep,
        parse_def_stall,
        parse_def_return,
        parse_def_unload,
        parse_def_while
    };

    Err(AmlError::AmlInvalidOpCode)
}

fn parse_def_break(data: &[u8],
                   ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0xA5);
    ctx.state = ExecutionState::BREAK;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 1 as usize
    })
}

fn parse_def_breakpoint(data: &[u8],
                        ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0xCC);

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 1 as usize
    })
}

fn parse_def_continue(data: &[u8],
                      ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x9F);
    ctx.state = ExecutionState::CONTINUE;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 1 as usize
    })
}

fn parse_def_noop(data: &[u8],
                  ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0xA3);

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 1 as usize
    })
}

fn parse_def_fatal(data: &[u8],
                   ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x32);

    let fatal_type = data[2];
    let fatal_code: u16 = (data[3] as u16) + ((data[4] as u16) << 8);
    let fatal_arg = parse_term_arg(&data[5..], ctx)?;

    Err(AmlError::AmlFatalError(fatal_type, fatal_code, fatal_arg.val))
}

fn parse_def_load(data: &[u8],
                  ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x20);

    let name = parse_name_string(&data[2..], ctx)?;
    let ddb_handle_object = parse_super_name(&data[2 + name.len..], ctx)?;

    let tbl = ctx.get(name.val)?.get_as_buffer()?;
    let sdt = unsafe { &*(tbl.as_ptr() as *const Sdt) };

    if is_aml_table(sdt) {
        load_table(get_sdt_signature(sdt));
        let delta = parse_aml_table(sdt)?;
        let _ = ctx.modify(ddb_handle_object.val, AmlValue::DDBHandle((delta, get_sdt_signature(sdt))));

        Ok(AmlParseType {
            val: AmlValue::None,
            len: 2 + name.len + ddb_handle_object.len
        })
    } else {
        Err(AmlError::AmlValueError)
    }
}

fn parse_def_notify(data: &[u8],
                    ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x86);

    let object = parse_super_name(&data[1..], ctx)?;
    let value = parse_term_arg(&data[1 + object.len..], ctx)?;

    let number = value.val.get_as_integer()? as u8;

    match ctx.get(object.val)? {
        AmlValue::Device(d) => {
            if let Some(methods) = d.notify_methods.get(&number) {
                for method in methods {
                    method();
                }
            }
        },
        AmlValue::Processor(d) => {
            if let Some(methods) = d.notify_methods.get(&number) {
                for method in methods {
                    method();
                }
            }
        },
        AmlValue::ThermalZone(d) => {
            if let Some(methods) = d.notify_methods.get(&number) {
                for method in methods {
                    method();
                }
            }
        },
        _ => return Err(AmlError::AmlValueError)
    }

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 1 + object.len + value.len
    })
}

fn parse_def_release(data: &[u8],
                     ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x27);

    let obj = parse_super_name(&data[2..], ctx)?;
    let _ = ctx.release_mutex(obj.val);

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + obj.len
    })
}

fn parse_def_reset(data: &[u8],
                   ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x26);

    let object = parse_super_name(&data[2..], ctx)?;
    ctx.get(object.val.clone())?.get_as_event()?;

    let _ = ctx.modify(object.val.clone(), AmlValue::Event(0));

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + object.len
    })
}

fn parse_def_signal(data: &[u8],
                    ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x24);
    let object = parse_super_name(&data[2..], ctx)?;

    ctx.signal_event(object.val)?;
    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + object.len
    })
}

fn parse_def_sleep(data: &[u8],
                   ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x22);

    let time = parse_term_arg(&data[2..], ctx)?;
    let timeout = time.val.get_as_integer()?;

    let (seconds, nanoseconds) = monotonic();
    let starting_time_ns = nanoseconds + (seconds * 1_000_000_000);

    loop {
        let (seconds, nanoseconds) = monotonic();
        let current_time_ns = nanoseconds + (seconds * 1_000_000_000);

        if current_time_ns - starting_time_ns > timeout as u64 * 1_000_000 {
            break;
        }
    }

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + time.len
    })
}

fn parse_def_stall(data: &[u8],
                   ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x21);

    let time = parse_term_arg(&data[2..], ctx)?;
    let timeout = time.val.get_as_integer()?;

    let (seconds, nanoseconds) = monotonic();
    let starting_time_ns = nanoseconds + (seconds * 1_000_000_000);

    loop {
        let (seconds, nanoseconds) = monotonic();
        let current_time_ns = nanoseconds + (seconds * 1_000_000_000);

        if current_time_ns - starting_time_ns > timeout as u64 * 1000 {
            break;
        }
    }

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + time.len
    })
}

fn parse_def_unload(data: &[u8],
                    ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x2A);

    let object = parse_super_name(&data[2..], ctx)?;

    let delta = ctx.get(object.val)?.get_as_ddb_handle()?;
    let mut namespace = ctx.prelock();

    if let Some(ref mut ns) = *namespace {
        for o in delta.0 {
            ns.remove(&o);
        }
    }

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + object.len
    })
}

fn parse_def_if_else(data: &[u8],
                     ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0xA0);

    let (pkg_length, pkg_length_len) = parse_pkg_length(&data[1..])?;
    let if_condition = parse_term_arg(&data[1 + pkg_length_len .. 1 + pkg_length], ctx)?;

    let (else_length, else_length_len) = if data.len() > 1 + pkg_length && data[1 + pkg_length] == 0xA1 {
        parse_pkg_length(&data[2 + pkg_length..])?
    } else {
        (0 as usize, 0 as usize)
    };

    if if_condition.val.get_as_integer()? > 0 {
        parse_term_list(&data[1 + pkg_length_len + if_condition.len .. 1 + pkg_length], ctx)?;
    } else if else_length > 0 {
        parse_term_list(&data[2 + pkg_length + else_length_len .. 2 + pkg_length + else_length], ctx)?;
    }

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 1 + pkg_length + if else_length > 0 { 1 + else_length } else { 0 }
    })
}

fn parse_def_while(data: &[u8],
                   ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0xA2);

    let (pkg_length, pkg_length_len) = parse_pkg_length(&data[1..])?;

    loop {
        let predicate = parse_term_arg(&data[1 + pkg_length_len..], ctx)?;
        if predicate.val.get_as_integer()? == 0 {
            break;
        }

        parse_term_list(&data[1 + pkg_length_len + predicate.len .. 1 + pkg_length], ctx)?;

        match ctx.state {
            ExecutionState::EXECUTING => (),
            ExecutionState::BREAK => {
                ctx.state = ExecutionState::EXECUTING;
                break;
            },
            ExecutionState::CONTINUE => ctx.state = ExecutionState::EXECUTING,
            _ => return Ok(AmlParseType {
                val: AmlValue::None,
                len: 0
            })
        }
    }

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 1 + pkg_length
    })
}

fn parse_def_return(data: &[u8],
                    ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0xA4);

    let arg_object = parse_term_arg(&data[1..], ctx)?;
    ctx.state = ExecutionState::RETURN(arg_object.val);

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 1 + arg_object.len
    })
}
