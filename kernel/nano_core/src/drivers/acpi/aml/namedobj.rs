use alloc::boxed::Box;
use alloc::string::String;
use alloc::btree_map::BTreeMap;

use super::AmlError;
use super::parser::{ AmlParseType, ParseResult, AmlParseTypeGeneric, AmlExecutionContext, ExecutionState };
use super::namespace::{AmlValue, FieldSelector, Method, get_namespace_string,
                       Accessor, BufferField, FieldUnit, Processor, PowerResource, OperationRegion,
                       Device, ThermalZone};
use super::namestring::{parse_name_string, parse_name_seg};
use super::termlist::{parse_term_arg, parse_object_list};
use super::pkglength::parse_pkg_length;
use super::type2opcode::parse_def_buffer;

#[derive(Debug, Copy, Clone)]
pub enum RegionSpace {
    SystemMemory,
    SystemIO,
    PCIConfig,
    EmbeddedControl,
    SMBus,
    SystemCMOS,
    PciBarTarget,
    IPMI,
    GeneralPurposeIO,
    GenericSerialBus,
    UserDefined(u8)
}

#[derive(Debug, Clone)]
pub struct FieldFlags {
    access_type: AccessType,
    lock_rule: bool,
    update_rule: UpdateRule
}

#[derive(Debug, Clone)]
pub enum AccessType {
    AnyAcc,
    ByteAcc,
    WordAcc,
    DWordAcc,
    QWordAcc,
    BufferAcc(AccessAttrib)
}

#[derive(Debug, Clone)]
pub enum UpdateRule {
    Preserve,
    WriteAsOnes,
    WriteAsZeros
}

#[derive(Debug, Clone)]
pub struct NamedField {
    name: String,
    length: usize
}

#[derive(Debug, Clone)]
pub struct AccessField {
    access_type: AccessType,
    access_attrib: AccessAttrib
}

#[derive(Debug, Clone)]
pub enum AccessAttrib {
    AttribBytes(u8),
    AttribRawBytes(u8),
    AttribRawProcessBytes(u8),
    AttribQuick,
    AttribSendReceive,
    AttribByte,
    AttribWord,
    AttribBlock,
    AttribProcessCall,
    AttribBlockProcessCall
}

pub fn parse_named_obj(data: &[u8],
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
        parse_def_bank_field,
        parse_def_create_bit_field,
        parse_def_create_byte_field,
        parse_def_create_word_field,
        parse_def_create_dword_field,
        parse_def_create_qword_field,
        parse_def_create_field,
        parse_def_data_region,
        parse_def_event,
        parse_def_external,
        parse_def_device,
        parse_def_op_region,
        parse_def_field,
        parse_def_index_field,
        parse_def_method,
        parse_def_mutex,
        parse_def_power_res,
        parse_def_processor,
        parse_def_thermal_zone
    };

    Err(AmlError::AmlInvalidOpCode)
}

fn parse_def_bank_field(data: &[u8],
                        ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x87);

    let (pkg_length, pkg_length_len) = parse_pkg_length(&data[2..])?;
    let data = &data[2 + pkg_length_len .. 2 + pkg_length];

    let region_name = parse_name_string(data, ctx)?;
    let bank_name = parse_name_string(&data[2 + pkg_length_len + region_name.len .. 2 + pkg_length], ctx)?;

    let bank_value = parse_term_arg(&data[2 + pkg_length_len + region_name.len .. 2 + pkg_length], ctx)?;

    let flags_raw = data[2 + pkg_length_len + region_name.len + bank_name.len + bank_value.len];
    let mut flags = FieldFlags {
        access_type: match flags_raw & 0x0F {
            0 => AccessType::AnyAcc,
            1 => AccessType::ByteAcc,
            2 => AccessType::WordAcc,
            3 => AccessType::DWordAcc,
            4 => AccessType::QWordAcc,
            5 => AccessType::BufferAcc(AccessAttrib::AttribByte),
            _ => return Err(AmlError::AmlParseError("BankField - invalid access type"))
        },
        lock_rule: (flags_raw & 0x10) == 0x10,
        update_rule: match (flags_raw & 0x60) >> 5 {
            0 => UpdateRule::Preserve,
            1 => UpdateRule::WriteAsOnes,
            2 => UpdateRule::WriteAsZeros,
            _ => return Err(AmlError::AmlParseError("BankField - invalid update rule"))
        }
    };

    let selector = FieldSelector::Bank {
        region: region_name.val.get_as_string()?,
        bank_register: bank_name.val.get_as_string()?,
        bank_selector: Box::new(bank_value.val)
    };

    parse_field_list(&data[3 + pkg_length_len + region_name.len + bank_name.len + bank_value.len ..
                           2 + pkg_length], ctx, selector, &mut flags)?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + pkg_length
    })
}

fn parse_def_create_bit_field(data: &[u8],
                              ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x8D);

    let source_buf = parse_term_arg(&data[2..], ctx)?;
    let bit_index = parse_term_arg(&data[2 + source_buf.len..], ctx)?;
    let name = parse_name_string(&data[1 + source_buf.len + bit_index.len..], ctx)?;

    let local_scope_string = get_namespace_string(ctx.scope.clone(), name.val)?;

    ctx.add_to_namespace(local_scope_string, AmlValue::BufferField(BufferField {
        source_buf: Box::new(source_buf.val),
        index: Box::new(bit_index.val),
        length: Box::new(AmlValue::IntegerConstant(1))
    }))?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + name.len + source_buf.len + bit_index.len
    })
}

fn parse_def_create_byte_field(data: &[u8],
                               ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x8C);

    let source_buf = parse_term_arg(&data[2..], ctx)?;
    let bit_index = parse_term_arg(&data[2 + source_buf.len..], ctx)?;
    let name = parse_name_string(&data[1 + source_buf.len + bit_index.len..], ctx)?;

    let local_scope_string = get_namespace_string(ctx.scope.clone(), name.val)?;

    ctx.add_to_namespace(local_scope_string, AmlValue::BufferField(BufferField {
        source_buf: Box::new(source_buf.val),
        index: Box::new(bit_index.val),
        length: Box::new(AmlValue::IntegerConstant(8))
    }))?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + name.len + source_buf.len + bit_index.len
    })
}

fn parse_def_create_word_field(data: &[u8],
                               ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x8B);

    let source_buf = parse_term_arg(&data[2..], ctx)?;
    let bit_index = parse_term_arg(&data[2 + source_buf.len..], ctx)?;
    let name = parse_name_string(&data[1 + source_buf.len + bit_index.len..], ctx)?;

    let local_scope_string = get_namespace_string(ctx.scope.clone(), name.val)?;

    ctx.add_to_namespace(local_scope_string, AmlValue::BufferField(BufferField {
        source_buf: Box::new(source_buf.val),
        index: Box::new(bit_index.val),
        length: Box::new(AmlValue::IntegerConstant(16))
    }))?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + name.len + source_buf.len + bit_index.len
    })
}

fn parse_def_create_dword_field(data: &[u8],
                                ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x8A);

    let source_buf = parse_term_arg(&data[2..], ctx)?;
    let bit_index = parse_term_arg(&data[2 + source_buf.len..], ctx)?;
    let name = parse_name_string(&data[1 + source_buf.len + bit_index.len..], ctx)?;

    let local_scope_string = get_namespace_string(ctx.scope.clone(), name.val)?;

    let _ = ctx.add_to_namespace(local_scope_string, AmlValue::BufferField(BufferField {
        source_buf: Box::new(source_buf.val),
        index: Box::new(bit_index.val),
        length: Box::new(AmlValue::IntegerConstant(32))
    }));

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + name.len + source_buf.len + bit_index.len
    })
}

fn parse_def_create_qword_field(data: &[u8],
                                ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x8F);

    let source_buf = parse_term_arg(&data[2..], ctx)?;
    let bit_index = parse_term_arg(&data[2 + source_buf.len..], ctx)?;
    let name = parse_name_string(&data[1 + source_buf.len + bit_index.len..], ctx)?;

    let local_scope_string = get_namespace_string(ctx.scope.clone(), name.val)?;

    ctx.add_to_namespace(local_scope_string, AmlValue::BufferField(BufferField {
        source_buf: Box::new(source_buf.val),
        index: Box::new(bit_index.val),
        length: Box::new(AmlValue::IntegerConstant(64))
    }))?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + name.len + source_buf.len + bit_index.len
    })
}

fn parse_def_create_field(data: &[u8],
                          ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x13);

    let source_buf = parse_term_arg(&data[2..], ctx)?;
    let bit_index = parse_term_arg(&data[2 + source_buf.len..], ctx)?;
    let num_bits = parse_term_arg(&data[2 + source_buf.len + bit_index.len..], ctx)?;
    let name = parse_name_string(&data[2 + source_buf.len + bit_index.len + num_bits.len..], ctx)?;

    let local_scope_string = get_namespace_string(ctx.scope.clone(), name.val)?;

    ctx.add_to_namespace(local_scope_string, AmlValue::BufferField(BufferField {
        source_buf: Box::new(source_buf.val),
        index: Box::new(bit_index.val),
        length: Box::new(num_bits.val)
    }))?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + name.len + source_buf.len + bit_index.len + num_bits.len
    })
}

fn parse_def_data_region(data: &[u8],
                         ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    // TODO: Find the actual offset and length, once table mapping is implemented
    parser_opcode_extended!(data, 0x88);

    let name = parse_name_string(&data[2..], ctx)?;
    let signature = parse_term_arg(&data[2 + name.len..], ctx)?;
    let oem_id = parse_term_arg(&data[2 + name.len + signature.len..], ctx)?;
    let oem_table_id = parse_term_arg(&data[2 + name.len + signature.len + oem_id.len..], ctx)?;

    let local_scope_string = get_namespace_string(ctx.scope.clone(), name.val)?;

    ctx.add_to_namespace(local_scope_string, AmlValue::OperationRegion(OperationRegion {
        region: RegionSpace::SystemMemory,
        offset: Box::new(AmlValue::IntegerConstant(0)),
        len: Box::new(AmlValue::IntegerConstant(0)),
        accessor: Accessor {
            read: |_x| 0 as u64,
            write: |_x, _y| ()
        },
        accessed_by: None
    }))?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + name.len + signature.len + oem_id.len + oem_table_id.len
    })
}

fn parse_def_event(data: &[u8],
                   ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x02);

    let name = parse_name_string(&data[2..], ctx)?;

    let local_scope_string = get_namespace_string(ctx.scope.clone(), name.val)?;
    ctx.add_to_namespace(local_scope_string, AmlValue::Event(0))?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + name.len
    })
}

fn parse_def_device(data: &[u8],
                    ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    // TODO: How to handle local context deferreds
    parser_opcode_extended!(data, 0x82);

    let (pkg_length, pkg_length_len) = parse_pkg_length(&data[2..])?;
    let name = parse_name_string(&data[2 + pkg_length_len .. 2 + pkg_length], ctx)?;

    let local_scope_string = get_namespace_string(ctx.scope.clone(), name.val)?;
    let mut local_ctx = AmlExecutionContext::new(local_scope_string.clone());

    parse_object_list(&data[2 + pkg_length_len + name.len .. 2 + pkg_length], &mut local_ctx)?;

    ctx.add_to_namespace(local_scope_string, AmlValue::Device(Device {
        obj_list: local_ctx.namespace_delta.clone(),
        notify_methods: BTreeMap::new()
    }))?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + pkg_length
    })
}

fn parse_def_op_region(data: &[u8],
                       ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x80);

    let name = parse_name_string(&data[2..], ctx)?;
    let region = match data[2 + name.len] {
        0x00 => RegionSpace::SystemMemory,
        0x01 => RegionSpace::SystemIO,
        0x02 => RegionSpace::PCIConfig,
        0x03 => RegionSpace::EmbeddedControl,
        0x04 => RegionSpace::SMBus,
        0x05 => RegionSpace::SystemCMOS,
        0x06 => RegionSpace::PciBarTarget,
        0x07 => RegionSpace::IPMI,
        0x08 => RegionSpace::GeneralPurposeIO,
        0x09 => RegionSpace::GenericSerialBus,
        0x80 ... 0xFF => RegionSpace::UserDefined(data[2 + name.len]),
        _ => return Err(AmlError::AmlParseError("OpRegion - invalid region"))
    };

    let offset = parse_term_arg(&data[3 + name.len..], ctx)?;
    let len = parse_term_arg(&data[3 + name.len + offset.len..], ctx)?;

    let local_scope_string = get_namespace_string(ctx.scope.clone(), name.val)?;
    ctx.add_to_namespace(local_scope_string, AmlValue::OperationRegion(OperationRegion {
        region: region,
        offset: Box::new(offset.val),
        len: Box::new(len.val),
        accessor: Accessor {
            read: |_x| 0 as u64,
            write: |_x, _y| ()
        },
        accessed_by: None
    }))?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 3 + name.len + offset.len + len.len
    })
}

fn parse_def_field(data: &[u8],
                   ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x81);

    let (pkg_length, pkg_length_len) = parse_pkg_length(&data[2..])?;
    let name = parse_name_string(&data[2 + pkg_length_len .. 2 + pkg_length], ctx)?;

    let flags_raw = data[2 + pkg_length_len + name.len];
    let mut flags = FieldFlags {
        access_type: match flags_raw & 0x0F {
            0 => AccessType::AnyAcc,
            1 => AccessType::ByteAcc,
            2 => AccessType::WordAcc,
            3 => AccessType::DWordAcc,
            4 => AccessType::QWordAcc,
            5 => AccessType::BufferAcc(AccessAttrib::AttribByte),
            _ => return Err(AmlError::AmlParseError("Field - Invalid access type"))
        },
        lock_rule: (flags_raw & 0x10) == 0x10,
        update_rule: match (flags_raw & 0x60) >> 5 {
            0 => UpdateRule::Preserve,
            1 => UpdateRule::WriteAsOnes,
            2 => UpdateRule::WriteAsZeros,
            _ => return Err(AmlError::AmlParseError("Field - Invalid update rule"))
        }
    };

    let selector = FieldSelector::Region(name.val.get_as_string()?);

    parse_field_list(&data[3 + pkg_length_len + name.len .. 2 + pkg_length], ctx, selector, &mut flags)?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + pkg_length
    })
}

fn parse_def_index_field(data: &[u8],
                         ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x86);

    let (pkg_length, pkg_length_len) = parse_pkg_length(&data[2..])?;
    let idx_name = parse_name_string(&data[2 + pkg_length_len .. 2 + pkg_length], ctx)?;
    let data_name = parse_name_string(&data[2 + pkg_length_len + idx_name.len .. 2 + pkg_length], ctx)?;

    let flags_raw = data[2 + pkg_length_len + idx_name.len + data_name.len];
    let mut flags = FieldFlags {
        access_type: match flags_raw & 0x0F {
            0 => AccessType::AnyAcc,
            1 => AccessType::ByteAcc,
            2 => AccessType::WordAcc,
            3 => AccessType::DWordAcc,
            4 => AccessType::QWordAcc,
            5 => AccessType::BufferAcc(AccessAttrib::AttribByte),
            _ => return Err(AmlError::AmlParseError("IndexField - Invalid access type"))
        },
        lock_rule: (flags_raw & 0x10) == 0x10,
        update_rule: match (flags_raw & 0x60) >> 5 {
            0 => UpdateRule::Preserve,
            1 => UpdateRule::WriteAsOnes,
            2 => UpdateRule::WriteAsZeros,
            _ => return Err(AmlError::AmlParseError("IndexField - Invalid update rule"))
        }
    };

    let selector = FieldSelector::Index {
        index_selector: idx_name.val.get_as_string()?,
        data_selector: data_name.val.get_as_string()?
    };

    parse_field_list(&data[3 + pkg_length_len + idx_name.len + data_name.len .. 2 + pkg_length],
                     ctx, selector, &mut flags)?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + pkg_length
    })
}

fn parse_field_list(data: &[u8],
                    ctx: &mut AmlExecutionContext,
                    selector: FieldSelector,
                    flags: &mut FieldFlags) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    let mut current_offset: usize = 0;
    let mut field_offset: usize = 0;
    let mut connection = AmlValue::Uninitialized;

    while current_offset < data.len() {
        let res = parse_field_element(&data[current_offset..], ctx, selector.clone(), &mut connection, flags, &mut field_offset)?;

        match ctx.state {
            ExecutionState::EXECUTING => (),
            _ => return Ok(AmlParseType {
                val: AmlValue::None,
                len: 0
            })
        }

        current_offset += res.len;
    }

    Ok(AmlParseType {
        val: AmlValue::None,
        len: data.len()
    })
}

fn parse_field_element(data: &[u8],
                       ctx: &mut AmlExecutionContext,
                       selector: FieldSelector,
                       connection: &mut AmlValue,
                       flags: &mut FieldFlags,
                       offset: &mut usize) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    let length = if let Ok(field) = parse_named_field(data, ctx) {
        let local_scope_string = get_namespace_string(ctx.scope.clone(), AmlValue::String(field.val.name.clone()))?;

        ctx.add_to_namespace(local_scope_string, AmlValue::FieldUnit(FieldUnit {
            selector: selector.clone(),
            connection: Box::new(connection.clone()),
            flags: flags.clone(),
            offset: offset.clone(),
            length: field.val.length
        }))?;

        *offset += field.val.length;
        field.len
    } else if let Ok(field) = parse_reserved_field(data, ctx) {
        *offset += field.val;
        field.len
    } else if let Ok(field) = parse_access_field(data, ctx) {
        match field.val.access_type {
            AccessType::BufferAcc(_) =>
                flags.access_type = AccessType::BufferAcc(field.val.access_attrib.clone()),
            ref a => flags.access_type = a.clone()
        }

        field.len
    } else if let Ok(field) = parse_connect_field(data, ctx) {
        *connection = field.val.clone();
        field.len
    } else {
        return Err(AmlError::AmlInvalidOpCode);
    };

    Ok(AmlParseType {
        val: AmlValue::None,
        len: length
    })
}

fn parse_named_field(data: &[u8], _ctx: &mut AmlExecutionContext) -> Result<AmlParseTypeGeneric<NamedField>, AmlError> {
    let (name_seg, name_seg_len) = parse_name_seg(&data[0..4])?;
    let name = match String::from_utf8(name_seg) {
        Ok(s) => s,
        Err(_) => return Err(AmlError::AmlParseError("NamedField - invalid name"))
    };
    let (length, length_len) = parse_pkg_length(&data[4..])?;

    Ok(AmlParseTypeGeneric {
        val: NamedField { name, length },
        len: name_seg_len + length_len
    })
}

fn parse_reserved_field(data: &[u8], _ctx: &mut AmlExecutionContext) -> Result<AmlParseTypeGeneric<usize>, AmlError> {
    parser_opcode!(data, 0x00);

    let (length, length_len) = parse_pkg_length(&data[1..])?;
    Ok(AmlParseTypeGeneric {
        val: length,
        len: 1 + length_len
    })
}

fn parse_access_field(data: &[u8], _ctx: &mut AmlExecutionContext) -> Result<AmlParseTypeGeneric<AccessField>, AmlError> {
    parser_opcode!(data, 0x01, 0x03);

    let flags_raw = data[1];
    let access_type = match flags_raw & 0x0F {
        0 => AccessType::AnyAcc,
        1 => AccessType::ByteAcc,
        2 => AccessType::WordAcc,
        3 => AccessType::DWordAcc,
        4 => AccessType::QWordAcc,
        5 => AccessType::BufferAcc(AccessAttrib::AttribByte),
        _ => return Err(AmlError::AmlParseError("AccessField - Invalid access type"))
    };

    let access_attrib = match (flags_raw & 0xC0) >> 6 {
        0 => match data[2] {
            0x02 => AccessAttrib::AttribQuick,
            0x04 => AccessAttrib::AttribSendReceive,
            0x06 => AccessAttrib::AttribByte,
            0x08 => AccessAttrib::AttribWord,
            0x0A => AccessAttrib::AttribBlock,
            0x0B => AccessAttrib::AttribBytes(data[3]),
            0x0C => AccessAttrib::AttribProcessCall,
            0x0D => AccessAttrib::AttribBlockProcessCall,
            0x0E => AccessAttrib::AttribRawBytes(data[3]),
            0x0F => AccessAttrib::AttribRawProcessBytes(data[3]),
            _ => return Err(AmlError::AmlParseError("AccessField - Invalid access attrib"))
        },
        1 => AccessAttrib::AttribBytes(data[2]),
        2 => AccessAttrib::AttribRawBytes(data[2]),
        3 => AccessAttrib::AttribRawProcessBytes(data[2]),
        _ => return Err(AmlError::AmlParseError("AccessField - Invalid access attrib"))
            // This should never happen but the compiler bitches if I don't cover this
    };

    Ok(AmlParseTypeGeneric {
        val: AccessField { access_type, access_attrib },
        len: if data[0] == 0x01 {
            3 as usize
        } else {
            4 as usize
        }
    })
}

fn parse_connect_field(data: &[u8],
                       ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x02);

    if let Ok(e) = parse_def_buffer(&data[1..], ctx) {
        Ok(AmlParseType {
            val: e.val,
            len: e.len + 1
        })
    } else {
        let name = parse_name_string(&data[1..], ctx)?;
        Ok(AmlParseType {
            val: AmlValue::Alias(name.val.get_as_string()?),
            len: name.len + 1
        })
    }
}

fn parse_def_method(data: &[u8],
                    ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode!(data, 0x14);

    let (pkg_len, pkg_len_len) = parse_pkg_length(&data[1..])?;
    let name = parse_name_string(&data[1 + pkg_len_len..], ctx)?;
    let flags = data[1 + pkg_len_len + name.len];

    let arg_count = flags & 0x07;
    let serialized = (flags & 0x08) == 0x08;
    let sync_level = flags & 0xF0 >> 4;

    let term_list = &data[2 + pkg_len_len + name.len .. 1 + pkg_len];

    let local_scope_string = get_namespace_string(ctx.scope.clone(), name.val)?;
    ctx.add_to_namespace(local_scope_string, AmlValue::Method(Method {
        arg_count,
        serialized,
        sync_level,
        term_list: term_list.to_vec()
    }))?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 1 + pkg_len
    })
}

fn parse_def_mutex(data: &[u8],
                   ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x01);

    let name = parse_name_string(&data[2 ..], ctx)?;
    let flags = data[2 + name.len];
    let sync_level = flags & 0x0F;

    let local_scope_string = get_namespace_string(ctx.scope.clone(), name.val)?;
    ctx.add_to_namespace(local_scope_string, AmlValue::Mutex((sync_level, None)))?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 3 + name.len
    })
}

fn parse_def_power_res(data: &[u8],
                       ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    // TODO: How to handle local context deferreds
    parser_opcode_extended!(data, 0x84);

    let (pkg_len, pkg_len_len) = parse_pkg_length(&data[2..])?;
    let name = parse_name_string(&data[2 + pkg_len_len..], ctx)?;

    let local_scope_string = get_namespace_string(ctx.scope.clone(), name.val)?;

    let system_level = data[2 + pkg_len_len + name.len];
    let resource_order: u16 = (data[3 + pkg_len_len + name.len] as u16) +
        ((data[4 + pkg_len_len + name.len] as u16) << 8);

    let mut local_ctx = AmlExecutionContext::new(local_scope_string.clone());
    parse_object_list(&data[5 + pkg_len_len + name.len .. 2 + pkg_len], &mut local_ctx)?;

    ctx.add_to_namespace(local_scope_string, AmlValue::PowerResource(PowerResource {
        system_level,
        resource_order,
        obj_list: local_ctx.namespace_delta.clone()
    }))?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + pkg_len
    })
}

fn parse_def_processor(data: &[u8],
                       ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x83);

    let (pkg_len, pkg_len_len) = parse_pkg_length(&data[2..])?;
    let name = parse_name_string(&data[2 + pkg_len_len..], ctx)?;

    let local_scope_string = get_namespace_string(ctx.scope.clone(), name.val)?;

    let proc_id = data[2 + pkg_len_len + name.len];
    let p_blk_addr: u32 = (data[3 + pkg_len_len + name.len] as u32) +
        ((data[4 + pkg_len_len + name.len] as u32) << 8) +
        ((data[5 + pkg_len_len + name.len] as u32) << 16) +
        ((data[6 + pkg_len_len + name.len] as u32) << 24);
    let p_blk_len = data[7 + pkg_len_len + name.len];

    let mut local_ctx = AmlExecutionContext::new(local_scope_string.clone());
    parse_object_list(&data[8 + pkg_len_len + name.len .. 2 + pkg_len], &mut local_ctx)?;

    ctx.add_to_namespace(local_scope_string, AmlValue::Processor(Processor {
        proc_id: proc_id,
        p_blk: if p_blk_len > 0 { Some(p_blk_addr) } else { None },
        obj_list: local_ctx.namespace_delta.clone(),
        notify_methods: BTreeMap::new()
    }))?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + pkg_len
    })
}

fn parse_def_thermal_zone(data: &[u8],
                          ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x85);

    let (pkg_len, pkg_len_len) = parse_pkg_length(&data[2..])?;
    let name = parse_name_string(&data[2 + pkg_len_len .. 2 + pkg_len], ctx)?;

    let local_scope_string = get_namespace_string(ctx.scope.clone(), name.val)?;

    let mut local_ctx = AmlExecutionContext::new(local_scope_string.clone());
    parse_object_list(&data[2 + pkg_len_len + name.len .. 2 + pkg_len], &mut local_ctx)?;

    ctx.add_to_namespace(local_scope_string, AmlValue::ThermalZone(ThermalZone {
        obj_list: local_ctx.namespace_delta.clone(),
        notify_methods: BTreeMap::new()
    }))?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 2 + pkg_len
    })
}

fn parse_def_external(data: &[u8],
                      ctx: &mut AmlExecutionContext) -> ParseResult {
    match ctx.state {
        ExecutionState::EXECUTING => (),
        _ => return Ok(AmlParseType {
            val: AmlValue::None,
            len: 0
        })
    }

    parser_opcode_extended!(data, 0x15);

    let object_name = parse_name_string(&data[1..], ctx)?;
    let object_type = data[1 + object_name.len];
    let argument_count = data[2 + object_name.len];

    let local_scope_string = get_namespace_string(ctx.scope.clone(), object_name.val)?;

    let obj = match object_type {
        8 => AmlValue::Method(Method {
            arg_count: argument_count,
            serialized: false,
            sync_level: 0,
            term_list: vec!()
        }),
        _ => AmlValue::Uninitialized
    };

    ctx.add_to_namespace(local_scope_string, obj)?;

    Ok(AmlParseType {
        val: AmlValue::None,
        len: 3 + object_name.len
    })
}
