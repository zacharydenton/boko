//! Amazon Ion binary format parser.
//!
//! Ion is Amazon's data serialization format used in KFX ebooks.
//! This implements a minimal parser sufficient for reading KFX content.
//!
//! Reference: https://amazon-ion.github.io/ion-docs/docs/binary.html

use std::collections::HashMap;
use std::io;

/// Ion binary version marker (BVM)
pub const ION_MAGIC: [u8; 4] = [0xe0, 0x01, 0x00, 0xea];

/// Ion type codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IonType {
    Null = 0,
    Bool = 1,
    PosInt = 2,
    NegInt = 3,
    Float = 4,
    Decimal = 5,
    Timestamp = 6,
    Symbol = 7,
    String = 8,
    Clob = 9,
    Blob = 10,
    List = 11,
    Sexp = 12,
    Struct = 13,
    Annotation = 14,
    Reserved = 15,
}

impl From<u8> for IonType {
    fn from(value: u8) -> Self {
        match value {
            0 => IonType::Null,
            1 => IonType::Bool,
            2 => IonType::PosInt,
            3 => IonType::NegInt,
            4 => IonType::Float,
            5 => IonType::Decimal,
            6 => IonType::Timestamp,
            7 => IonType::Symbol,
            8 => IonType::String,
            9 => IonType::Clob,
            10 => IonType::Blob,
            11 => IonType::List,
            12 => IonType::Sexp,
            13 => IonType::Struct,
            14 => IonType::Annotation,
            _ => IonType::Reserved,
        }
    }
}

/// Parsed Ion value
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum IonValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Symbol(u64),
    String(String),
    Blob(Vec<u8>),
    List(Vec<IonValue>),
    Struct(HashMap<u64, IonValue>),
    /// Annotated value: (annotation symbol IDs, inner value)
    Annotated(Vec<u64>, Box<IonValue>),
}

#[allow(dead_code)]
impl IonValue {
    /// Get as string if this is a String value
    pub fn as_string(&self) -> Option<&str> {
        match self {
            IonValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Get as i64 if this is an Int value
    pub fn as_int(&self) -> Option<i64> {
        match self {
            IonValue::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// Get as bool if this is a Bool value
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            IonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Get as symbol ID if this is a Symbol value
    pub fn as_symbol(&self) -> Option<u64> {
        match self {
            IonValue::Symbol(id) => Some(*id),
            _ => None,
        }
    }

    /// Get as list if this is a List value
    pub fn as_list(&self) -> Option<&[IonValue]> {
        match self {
            IonValue::List(items) => Some(items),
            _ => None,
        }
    }

    /// Get as struct if this is a Struct value
    pub fn as_struct(&self) -> Option<&HashMap<u64, IonValue>> {
        match self {
            IonValue::Struct(map) => Some(map),
            _ => None,
        }
    }

    /// Get field from struct by symbol ID
    pub fn get(&self, symbol_id: u64) -> Option<&IonValue> {
        self.as_struct()?.get(&symbol_id)
    }

    /// Unwrap annotated value to get inner value
    pub fn unwrap_annotated(&self) -> &IonValue {
        match self {
            IonValue::Annotated(_, inner) => inner.unwrap_annotated(),
            other => other,
        }
    }
}

/// Ion binary parser
pub struct IonParser<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> IonParser<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Parse Ion data starting with the BVM marker
    pub fn parse(&mut self) -> io::Result<IonValue> {
        // Check and skip Ion BVM
        if self.remaining() < 4 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "data too short"));
        }
        if &self.data[self.pos..self.pos + 4] != ION_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not Ion data (missing BVM)",
            ));
        }
        self.pos += 4;
        self.parse_value()
    }

    /// Parse a single Ion value
    pub fn parse_value(&mut self) -> io::Result<IonValue> {
        if self.remaining() == 0 {
            return Ok(IonValue::Null);
        }

        let type_byte = self.read_u8()?;
        let ion_type = IonType::from(type_byte >> 4);
        let length_code = type_byte & 0x0f;

        // Handle null (length code 15 means typed null)
        if length_code == 15 {
            return Ok(IonValue::Null);
        }

        // Get actual length
        let length = if length_code == 14 {
            // Variable length follows as VarUInt
            self.read_varuint()? as usize
        } else {
            length_code as usize
        };

        match ion_type {
            IonType::Null => Ok(IonValue::Null),

            IonType::Bool => {
                // For bool, length code IS the value (0 = false, 1 = true)
                Ok(IonValue::Bool(length_code != 0))
            }

            IonType::PosInt => {
                let value = self.read_uint(length)?;
                Ok(IonValue::Int(value as i64))
            }

            IonType::NegInt => {
                let value = self.read_uint(length)?;
                Ok(IonValue::Int(-(value as i64)))
            }

            IonType::Float => {
                let value = if length == 4 {
                    let bytes = self.read_bytes(4)?;
                    f32::from_be_bytes(bytes.try_into().unwrap()) as f64
                } else if length == 8 {
                    let bytes = self.read_bytes(8)?;
                    f64::from_be_bytes(bytes.try_into().unwrap())
                } else {
                    0.0
                };
                Ok(IonValue::Float(value))
            }

            IonType::Symbol => {
                let symbol_id = self.read_uint(length)?;
                Ok(IonValue::Symbol(symbol_id))
            }

            IonType::String => {
                let bytes = self.read_bytes(length)?;
                let s = String::from_utf8_lossy(bytes).into_owned();
                Ok(IonValue::String(s))
            }

            IonType::Blob | IonType::Clob => {
                let bytes = self.read_bytes(length)?.to_vec();
                Ok(IonValue::Blob(bytes))
            }

            IonType::List | IonType::Sexp => {
                let end = self.pos + length;
                let mut items = Vec::new();
                while self.pos < end {
                    items.push(self.parse_value()?);
                }
                Ok(IonValue::List(items))
            }

            IonType::Struct => {
                let end = self.pos + length;
                let mut fields = HashMap::new();
                while self.pos < end {
                    let field_name = self.read_varuint()?;
                    let value = self.parse_value()?;
                    fields.insert(field_name, value);
                }
                Ok(IonValue::Struct(fields))
            }

            IonType::Annotation => {
                let end = self.pos + length;

                // Read annotation length (VarUInt)
                let ann_len = self.read_varuint()? as usize;
                let ann_end = self.pos + ann_len;

                // Read annotation symbol IDs
                let mut annotations = Vec::new();
                while self.pos < ann_end {
                    annotations.push(self.read_varuint()?);
                }

                // Parse the annotated value
                let inner = if self.pos < end {
                    self.parse_value()?
                } else {
                    IonValue::Null
                };

                Ok(IonValue::Annotated(annotations, Box::new(inner)))
            }

            _ => {
                // Skip unknown types
                self.pos += length;
                Ok(IonValue::Null)
            }
        }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_u8(&mut self) -> io::Result<u8> {
        if self.pos >= self.data.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "unexpected end of data",
            ));
        }
        let byte = self.data[self.pos];
        self.pos += 1;
        Ok(byte)
    }

    fn read_bytes(&mut self, len: usize) -> io::Result<&'a [u8]> {
        if self.pos + len > self.data.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "unexpected end of data",
            ));
        }
        let bytes = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(bytes)
    }

    /// Read a VarUInt (7 bits per byte, MSB set on last byte)
    fn read_varuint(&mut self) -> io::Result<u64> {
        let mut result: u64 = 0;
        for _ in 0..10 {
            let byte = self.read_u8()?;
            result = (result << 7) | (byte & 0x7f) as u64;
            if byte & 0x80 != 0 {
                return Ok(result);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "VarUInt too long",
        ))
    }

    /// Read unsigned integer (big-endian)
    fn read_uint(&mut self, len: usize) -> io::Result<u64> {
        if len == 0 {
            return Ok(0);
        }
        let bytes = self.read_bytes(len)?;
        let mut result: u64 = 0;
        for &b in bytes {
            result = (result << 8) | b as u64;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bool() {
        // true: type=1, length=1
        let data = [0xe0, 0x01, 0x00, 0xea, 0x11];
        let mut parser = IonParser::new(&data);
        let value = parser.parse().unwrap();
        assert_eq!(value.as_bool(), Some(true));

        // false: type=1, length=0
        let data = [0xe0, 0x01, 0x00, 0xea, 0x10];
        let mut parser = IonParser::new(&data);
        let value = parser.parse().unwrap();
        assert_eq!(value.as_bool(), Some(false));
    }

    #[test]
    fn test_parse_int() {
        // positive int 42: type=2, length=1, value=42
        let data = [0xe0, 0x01, 0x00, 0xea, 0x21, 0x2a];
        let mut parser = IonParser::new(&data);
        let value = parser.parse().unwrap();
        assert_eq!(value.as_int(), Some(42));
    }

    #[test]
    fn test_parse_string() {
        // string "hi": type=8, length=2, value="hi"
        let data = [0xe0, 0x01, 0x00, 0xea, 0x82, b'h', b'i'];
        let mut parser = IonParser::new(&data);
        let value = parser.parse().unwrap();
        assert_eq!(value.as_string(), Some("hi"));
    }
}
