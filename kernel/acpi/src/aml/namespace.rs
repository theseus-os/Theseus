use alloc::boxed::Box;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use alloc::btree_map::BTreeMap;

use core::fmt::{Debug, Formatter, Error};
use core::str::FromStr;

use super::termlist::parse_term_list;
use super::namedobj::{ RegionSpace, FieldFlags };
use super::parser::{AmlExecutionContext, ExecutionState};
use super::AmlError;

use acpi::{SdtSignature, get_signature_from_index, get_index_from_signature};

#[derive(Clone)]
pub enum FieldSelector {
    Region(String),
    Bank {
        region: String,
        bank_register: String,
        bank_selector: Box<AmlValue>
    },
    Index {
        index_selector: String,
        data_selector: String
    }
}

#[derive(Clone)]
pub enum ObjectReference {
    ArgObj(u8),
    LocalObj(u8),
    Object(String),
    Index(Box<AmlValue>, Box<AmlValue>)
}

#[derive(Clone)]
pub struct Method {
    pub arg_count: u8,
    pub serialized: bool,
    pub sync_level: u8,
    pub term_list: Vec<u8>
}

#[derive(Clone)]
pub struct BufferField {
    pub source_buf: Box<AmlValue>,
    pub index: Box<AmlValue>,
    pub length: Box<AmlValue>
}

#[derive(Clone)]
pub struct FieldUnit {
    pub selector: FieldSelector,
    pub connection: Box<AmlValue>,
    pub flags: FieldFlags,
    pub offset: usize,
    pub length: usize
}

#[derive(Clone)]
pub struct Device {
    pub obj_list: Vec<String>,
    pub notify_methods: BTreeMap<u8, Vec<fn()>>
}

#[derive(Clone)]
pub struct ThermalZone {
    pub obj_list: Vec<String>,
    pub notify_methods: BTreeMap<u8, Vec<fn()>>
}

#[derive(Clone)]
pub struct Processor {
    pub proc_id: u8,
    pub p_blk: Option<u32>,
    pub obj_list: Vec<String>,
    pub notify_methods: BTreeMap<u8, Vec<fn()>>
}

#[derive(Clone)]
pub struct OperationRegion {
    pub region: RegionSpace,
    pub offset: Box<AmlValue>,
    pub len: Box<AmlValue>,
    pub accessor: Accessor,
    pub accessed_by: Option<u64>
}

#[derive(Clone)]
pub struct PowerResource {
    pub system_level: u8,
    pub resource_order: u16,
    pub obj_list: Vec<String>
}

pub struct Accessor {
    pub read: fn(usize) -> u64,
    pub write: fn(usize, u64)
}

impl Clone for Accessor {
    fn clone(&self) -> Accessor {
        Accessor {
            read: (*self).read,
            write: (*self).write
        }
    }
}

#[derive(Clone)]
pub enum AmlValue {
    None,
    Uninitialized,
    Alias(String),
    Buffer(Vec<u8>),
    BufferField(BufferField),
    DDBHandle((Vec<String>, SdtSignature)),
    DebugObject,
    Device(Device),
    Event(u64),
    FieldUnit(FieldUnit),
    Integer(u64),
    IntegerConstant(u64),
    Method(Method),
    Mutex((u8, Option<u64>)),
    ObjectReference(ObjectReference),
    OperationRegion(OperationRegion),
    Package(Vec<AmlValue>),
    String(String),
    PowerResource(PowerResource),
    Processor(Processor),
    RawDataBuffer(Vec<u8>),
    ThermalZone(ThermalZone)
}

impl Debug for AmlValue {
    fn fmt(&self, _f: &mut Formatter) -> Result<(), Error> { Ok(()) }
}

impl AmlValue {
    pub fn get_type_string(&self) -> String {
        match *self {
            AmlValue::Uninitialized => String::from_str("[Uninitialized Object]").unwrap(),
            AmlValue::Integer(_) => String::from_str("[Integer]").unwrap(),
            AmlValue::String(_) => String::from_str("[String]").unwrap(),
            AmlValue::Buffer(_) => String::from_str("[Buffer]").unwrap(),
            AmlValue::Package(_) => String::from_str("[Package]").unwrap(),
            AmlValue::FieldUnit(_) => String::from_str("[Field]").unwrap(),
            AmlValue::Device(_) => String::from_str("[Device]").unwrap(),
            AmlValue::Event(_) => String::from_str("[Event]").unwrap(),
            AmlValue::Method(_) => String::from_str("[Control Method]").unwrap(),
            AmlValue::Mutex(_) => String::from_str("[Mutex]").unwrap(),
            AmlValue::OperationRegion(_) => String::from_str("[Operation Region]").unwrap(),
            AmlValue::PowerResource(_) => String::from_str("[Power Resource]").unwrap(),
            AmlValue::Processor(_) => String::from_str("[Processor]").unwrap(),
            AmlValue::ThermalZone(_) => String::from_str("[Thermal Zone]").unwrap(),
            AmlValue::BufferField(_) => String::from_str("[Buffer Field]").unwrap(),
            AmlValue::DDBHandle(_) => String::from_str("[DDB Handle]").unwrap(),
            AmlValue::DebugObject => String::from_str("[Debug Object]").unwrap(),
            _ => String::new()
        }
    }

    pub fn get_as_type(&self, t: AmlValue) -> Result<AmlValue, AmlError> {
        match t {
            AmlValue::None => Ok(AmlValue::None),
            AmlValue::Uninitialized => Ok(self.clone()),
            AmlValue::Alias(_) => match *self {
                AmlValue::Alias(_) => Ok(self.clone()),
                _ => Err(AmlError::AmlValueError)
            },
            AmlValue::Buffer(_) => Ok(AmlValue::Buffer(self.get_as_buffer()?)),
            AmlValue::BufferField(_) => Ok(AmlValue::BufferField(self.get_as_buffer_field()?)),
            AmlValue::DDBHandle(_) => Ok(AmlValue::DDBHandle(self.get_as_ddb_handle()?)),
            AmlValue::DebugObject => match *self {
                AmlValue::DebugObject => Ok(self.clone()),
                _ => Err(AmlError::AmlValueError)
            },
            AmlValue::Device(_) => Ok(AmlValue::Device(self.get_as_device()?)),
            AmlValue::Event(_) => Ok(AmlValue::Event(self.get_as_event()?)),
            AmlValue::FieldUnit(_) => Ok(AmlValue::FieldUnit(self.get_as_field_unit()?)),
            AmlValue::Integer(_) => Ok(AmlValue::Integer(self.get_as_integer()?)),
            AmlValue::IntegerConstant(_) => Ok(AmlValue::IntegerConstant(self.get_as_integer_constant()?)),
            AmlValue::Method(_) => Ok(AmlValue::Method(self.get_as_method()?)),
            AmlValue::Mutex(_) => Ok(AmlValue::Mutex(self.get_as_mutex()?)),
            AmlValue::ObjectReference(_) => Ok(AmlValue::ObjectReference(self.get_as_object_reference()?)),
            AmlValue::OperationRegion(_) => match *self {
                AmlValue::OperationRegion(_) => Ok(self.clone()),
                _ => Err(AmlError::AmlValueError)
            },
            AmlValue::Package(_) => Ok(AmlValue::Package(self.get_as_package()?)),
            AmlValue::String(_) => Ok(AmlValue::String(self.get_as_string()?)),
            AmlValue::PowerResource(_) => Ok(AmlValue::PowerResource(self.get_as_power_resource()?)),
            AmlValue::Processor(_) => Ok(AmlValue::Processor(self.get_as_processor()?)),
            AmlValue::RawDataBuffer(_) => Ok(AmlValue::RawDataBuffer(self.get_as_raw_data_buffer()?)),
            AmlValue::ThermalZone(_) => Ok(AmlValue::ThermalZone(self.get_as_thermal_zone()?))
        }
    }

    pub fn get_as_buffer(&self) -> Result<Vec<u8>, AmlError> {
        match *self {
            AmlValue::Buffer(ref b) => Ok(b.clone()),
            AmlValue::Integer(ref i) => {
                let mut v: Vec<u8> = vec!();
                let mut i = i.clone();

                while i != 0 {
                    v.push((i & 0xFF) as u8);
                    i >>= 8;
                }

                while v.len() < 8 {
                    v.push(0);
                }

                Ok(v)
            },
            AmlValue::String(ref s) => {
                Ok(s.clone().into_bytes())
            },
            AmlValue::BufferField(ref b) => {
                let buf = b.source_buf.get_as_buffer()?;
                let idx = b.index.get_as_integer()? as usize;
                let len = b.length.get_as_integer()? as usize;

                if idx + len > buf.len() {
                    return Err(AmlError::AmlValueError);
                }

                Ok(buf[idx .. idx + len].to_vec())
            },
            _ => Err(AmlError::AmlValueError)
        }
    }

    pub fn get_as_buffer_field(&self) -> Result<BufferField, AmlError> {
        match *self {
            AmlValue::BufferField(ref b) => Ok(b.clone()),
            _ => {
                let raw_buf = self.get_as_buffer()?;
                let buf = Box::new(AmlValue::Buffer(raw_buf.clone()));
                let idx = Box::new(AmlValue::IntegerConstant(0));
                let len = Box::new(AmlValue::Integer(raw_buf.len() as u64));

                Ok(BufferField {
                    source_buf: buf,
                    index: idx,
                    length: len
                })
            }
        }
    }

    pub fn get_as_ddb_handle(&self) -> Result<(Vec<String>, SdtSignature), AmlError> {
        match *self {
            AmlValue::DDBHandle(ref v) => Ok(v.clone()),
            AmlValue::Integer(i) => if let Some(sig) = get_signature_from_index(i as usize) {
                Ok((vec!(), sig))
            } else {
                Err(AmlError::AmlValueError)
            },
            _ => Err(AmlError::AmlValueError)
        }
    }

    pub fn get_as_device(&self) -> Result<Device, AmlError> {
        match *self {
            AmlValue::Device(ref s) => Ok(s.clone()),
            _ => Err(AmlError::AmlValueError)
        }
    }

    pub fn get_as_event(&self) -> Result<u64, AmlError> {
        match *self {
            AmlValue::Event(ref e) => Ok(e.clone()),
            _ => Err(AmlError::AmlValueError)
        }
    }

    pub fn get_as_field_unit(&self) -> Result<FieldUnit, AmlError> {
        match *self {
            AmlValue::FieldUnit(ref e) => Ok(e.clone()),
            _ => Err(AmlError::AmlValueError)
        }
    }

    pub fn get_as_integer(&self) -> Result<u64, AmlError> {
        match *self {
            AmlValue::IntegerConstant(ref i) => Ok(i.clone()),
            AmlValue::Integer(ref i) => Ok(i.clone()),
            AmlValue::Buffer(ref b) => {
                let mut b = b.clone();
                if b.len() > 8 {
                    return Err(AmlError::AmlValueError);
                }

                let mut i: u64 = 0;

                while b.len() > 0 {
                    i <<= 8;
                    i += b.pop().expect("Won't happen") as u64;
                }

                Ok(i)
            },
            AmlValue::BufferField(_) => {
                let mut b = self.get_as_buffer()?;
                if b.len() > 8 {
                    return Err(AmlError::AmlValueError);
                }

                let mut i: u64 = 0;

                while b.len() > 0 {
                    i <<= 8;
                    i += b.pop().expect("Won't happen") as u64;
                }

                Ok(i)
            },
            AmlValue::DDBHandle(ref v) => if let Some(idx) = get_index_from_signature(v.1.clone()) {
                Ok(idx as u64)
            } else {
                Err(AmlError::AmlValueError)
            },
            AmlValue::String(ref s) => {
                let s = s.clone()[0..8].to_string().to_uppercase();
                let mut i: u64 = 0;

                for c in s.chars() {
                    if !c.is_digit(16) {
                        break;
                    }

                    i <<= 8;
                    i += c.to_digit(16).unwrap() as u64;
                }

                Ok(i)
            },
            _ => Err(AmlError::AmlValueError)
        }
    }

    pub fn get_as_integer_constant(&self) -> Result<u64, AmlError> {
        match *self {
            AmlValue::IntegerConstant(ref i) => Ok(i.clone()),
            _ => Err(AmlError::AmlValueError)
        }
    }

    pub fn get_as_method(&self) -> Result<Method, AmlError> {
        match *self {
            AmlValue::Method(ref m) => Ok(m.clone()),
            _ => Err(AmlError::AmlValueError)
        }
    }

    pub fn get_as_mutex(&self) -> Result<(u8, Option<u64>), AmlError> {
        match *self {
            AmlValue::Mutex(ref m) => Ok(m.clone()),
            _ => Err(AmlError::AmlValueError)
        }
    }

    pub fn get_as_object_reference(&self) -> Result<ObjectReference, AmlError> {
        match *self {
            AmlValue::ObjectReference(ref m) => Ok(m.clone()),
            _ => Err(AmlError::AmlValueError)
        }
    }

    /*
    pub fn get_as_operation_region(&self) -> Result<OperationRegion, AmlError> {
        match *self {
            AmlValue::OperationRegion(ref p) => Ok(p.clone()),
            _ => Err(AmlError::AmlValueError)
        }
    }
    */

    pub fn get_as_package(&self) -> Result<Vec<AmlValue>, AmlError> {
        match *self {
            AmlValue::Package(ref p) => Ok(p.clone()),
            _ => Err(AmlError::AmlValueError)
        }
    }

    pub fn get_as_string(&self) -> Result<String, AmlError> {
        match *self {
            AmlValue::String(ref s) => Ok(s.clone()),
            AmlValue::Integer(ref i) => Ok(format!("{:X}", i)),
            AmlValue::IntegerConstant(ref i) => Ok(format!("{:X}", i)),
            AmlValue::Buffer(ref b) => Ok(String::from_utf8(b.clone()).expect("Invalid UTF-8")),
            AmlValue::BufferField(_) => {
                let b = self.get_as_buffer()?;
                Ok(String::from_utf8(b).expect("Invalid UTF-8"))
            },
            _ => Err(AmlError::AmlValueError)
        }
    }

    pub fn get_as_power_resource(&self) -> Result<PowerResource, AmlError> {
        match *self {
            AmlValue::PowerResource(ref p) => Ok(p.clone()),
            _ => Err(AmlError::AmlValueError)
        }
    }

    pub fn get_as_processor(&self) -> Result<Processor, AmlError> {
        match *self {
            AmlValue::Processor(ref p) => Ok(p.clone()),
            _ => Err(AmlError::AmlValueError)
        }
    }

    pub fn get_as_raw_data_buffer(&self) -> Result<Vec<u8>, AmlError> {
        match *self {
            AmlValue::RawDataBuffer(ref p) => Ok(p.clone()),
            _ => Err(AmlError::AmlValueError)
        }
    }

    pub fn get_as_thermal_zone(&self) -> Result<ThermalZone, AmlError> {
        match *self {
            AmlValue::ThermalZone(ref p) => Ok(p.clone()),
            _ => Err(AmlError::AmlValueError)
        }
    }
}

impl Method {
    pub fn execute(&self, scope: String, parameters: Vec<AmlValue>) -> AmlValue {
        let mut ctx = AmlExecutionContext::new(scope);
        ctx.init_arg_vars(parameters);

        let _ = parse_term_list(&self.term_list[..], &mut ctx);
        ctx.clean_namespace();

        match ctx.state {
            ExecutionState::RETURN(v) => v,
            _ => AmlValue::IntegerConstant(0)
        }
    }
}

pub fn get_namespace_string(current: String, modifier_v: AmlValue) -> Result<String, AmlError> {
    let mut modifier = modifier_v.get_as_string()?;

    if current.len() == 0 {
        return Ok(modifier);
    }

    if modifier.len() == 0 {
        return Ok(current);
    }

    if modifier.starts_with("\\") {
        return Ok(modifier);
    }

    let mut namespace = current.clone();

    if modifier.starts_with("^") {
        while modifier.starts_with("^") {
            modifier = modifier[1..].to_string();

            if namespace.ends_with("\\") {
                return Err(AmlError::AmlValueError);
            }

            loop {
                if namespace.ends_with(".") {
                    namespace.pop();
                    break;
                }

                if namespace.pop() == None {
                    return Err(AmlError::AmlValueError);
                }
            }
        }
    }

    if !namespace.ends_with("\\") {
        namespace.push('.');
    }

    Ok(namespace + &modifier)
}
