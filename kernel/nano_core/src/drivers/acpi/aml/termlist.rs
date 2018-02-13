use alloc::vec::Vec;

use super::AmlError;
use super::parser::{ AmlParseType, ParseResult, AmlExecutionContext, ExecutionState };
use super::namespace::{AmlValue, get_namespace_string};
use super::namespacemodifier::parse_namespace_modifier;
use super::namedobj::parse_named_obj;
use super::dataobj::{parse_data_obj, parse_arg_obj, parse_local_obj};
use super::type1opcode::parse_type1_opcode;
use super::type2opcode::parse_type2_opcode;
use super::namestring::parse_name_string;

pub fn parse_term_list(data: &[u8],
                       ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    let mut current_offset: usize = 0;

    while current_offset < data.len() {
        let res = parse_term_obj(&data[current_offset..], ctx)?;

        match ctx.state {
            ExecutionState::EXECUTING => (),
            _ => return Ok(AmlParseType {
                val: AmlValue::None,
                len: data.len()
            })
        }

        current_offset += res.len;
    }

    Ok(AmlParseType {
        val: AmlValue::None,
        len: data.len()
    })
}

pub fn parse_term_arg(data: &[u8],
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
        parse_local_obj,
        parse_data_obj,
        parse_arg_obj,
        parse_type2_opcode
    };

    Err(AmlError::AmlInvalidOpCode)
}

pub fn parse_object_list(data: &[u8],
                         ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    let mut current_offset: usize = 0;

    while current_offset < data.len() {
        let res = parse_object(&data[current_offset..], ctx)?;

        match ctx.state {
            ExecutionState::EXECUTING => (),
            _ => return Ok(AmlParseType {
                val: AmlValue::None,
                len: data.len()
            })
        }

        current_offset += res.len;
    }

    Ok(AmlParseType {
        val: AmlValue::None,
        len: data.len()
    })
}

fn parse_object(data: &[u8],
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
        parse_namespace_modifier,
        parse_named_obj
    };

    Err(AmlError::AmlInvalidOpCode)
}

pub fn parse_method_invocation(data: &[u8],
                               ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    let name = parse_name_string(data, ctx)?;
    let method = ctx.get(name.val.clone())?;

    let method = match method {
        AmlValue::None => return Err(AmlError::AmlDeferredLoad),
        _ => method.get_as_method()?
    };

    let mut cur = 0;
    let mut params: Vec<AmlValue> = vec!();

    let mut current_offset = name.len;

    while cur < method.arg_count {
        let res = parse_term_arg(&data[current_offset..], ctx)?;

        current_offset += res.len;
        cur += 1;

        params.push(res.val);
    }

    Ok(AmlParseType {
        val: method.execute(get_namespace_string(ctx.scope.clone(), name.val)?, params),
        len: current_offset
    })
}

fn parse_term_obj(data: &[u8],
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
        parse_namespace_modifier,
        parse_named_obj,
        parse_type1_opcode,
        parse_type2_opcode
    };

    Err(AmlError::AmlInvalidOpCode)
}
