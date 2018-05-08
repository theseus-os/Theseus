use super::AmlError;
use super::parser::{AmlParseType, ParseResult, AmlExecutionContext, ExecutionState};
use super::namespace::{AmlValue, get_namespace_string};
use super::pkglength::parse_pkg_length;
use super::namestring::parse_name_string;
use super::termlist::parse_term_list;
use super::dataobj::parse_data_ref_obj;

pub fn parse_namespace_modifier(data: &[u8],
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
        parse_alias_op,
        parse_scope_op,
        parse_name_op
    };

    Err(AmlError::AmlInvalidOpCode)
}

fn parse_alias_op(data: &[u8],
                  ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x06);

    let source_name = parse_name_string(&data[1..], ctx)?;
    let alias_name = parse_name_string(&data[1 + source_name.len..], ctx)?;

    let local_scope_string = get_namespace_string(ctx.scope.clone(), source_name.val)?;
    let local_alias_string = get_namespace_string(ctx.scope.clone(), alias_name.val)?;

    ctx.add_to_namespace(local_scope_string, AmlValue::Alias(local_alias_string))?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 1 + source_name.len + alias_name.len
    })
}

fn parse_name_op(data: &[u8],
                 ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x08);

    let name = parse_name_string(&data[1..], ctx)?;
    let data_ref_obj = parse_data_ref_obj(&data[1 + name.len..], ctx)?;

    let local_scope_string = get_namespace_string(ctx.scope.clone(), name.val)?;

    ctx.add_to_namespace(local_scope_string, data_ref_obj.val)?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 1 + name.len + data_ref_obj.len
    })
}

fn parse_scope_op(data: &[u8],
                  ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x10);

    let (pkg_length, pkg_length_len) = parse_pkg_length(&data[1..])?;
    let name = parse_name_string(&data[1 + pkg_length_len..], ctx)?;

    let local_scope_string = get_namespace_string(ctx.scope.clone(), name.val.clone())?;
    let containing_scope_string = ctx.scope.clone();

    ctx.scope = local_scope_string;
    parse_term_list(&data[1 + pkg_length_len + name.len .. 1 + pkg_length], ctx)?;
    ctx.scope = containing_scope_string;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 1 + pkg_length
    })
}
