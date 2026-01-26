//! Amazon Ion binary format parser.
//!
//! Ion is Amazon's data serialization format used in KFX ebooks.
//! This implements a minimal parser sufficient for reading KFX content.
//!
//! Reference: <https://amazon-ion.github.io/ion-docs/docs/binary.html>

use std::io;

/// Ion binary version marker (BVM)
pub const ION_MAGIC: [u8; 4] = [0xe0, 0x01, 0x00, 0xea];

/// Ion type codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum IonType {
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
}

impl IonType {
    fn from_nibble(n: u8) -> Option<Self> {
        match n {
            0 => Some(IonType::Null),
            1 => Some(IonType::Bool),
            2 => Some(IonType::PosInt),
            3 => Some(IonType::NegInt),
            4 => Some(IonType::Float),
            5 => Some(IonType::Decimal),
            6 => Some(IonType::Timestamp),
            7 => Some(IonType::Symbol),
            8 => Some(IonType::String),
            9 => Some(IonType::Clob),
            10 => Some(IonType::Blob),
            11 => Some(IonType::List),
            12 => Some(IonType::Sexp),
            13 => Some(IonType::Struct),
            14 => Some(IonType::Annotation),
            _ => None, // Reserved (15)
        }
    }
}

/// Parsed Ion value.
///
/// Symbols are stored as raw u32 IDs - use the KFX symbol table to resolve them.
/// Structs use a Vec for fields (O(n) lookup) which is optimal for small structs
/// typical in KFX data.
#[derive(Debug, Clone)]
pub enum IonValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// Symbol ID (resolve via KFX_SYMBOL_TABLE or doc_symbols)
    Symbol(u32),
    String(String),
    Blob(Vec<u8>),
    List(Vec<IonValue>),
    /// Struct fields as (symbol_id, value) pairs in parse order
    Struct(Vec<(u32, IonValue)>),
    /// Annotated value: (annotation symbol IDs, inner value)
    Annotated(Vec<u32>, Box<IonValue>),
}

impl IonValue {
    /// Get as string if this is a String value.
    #[inline]
    pub fn as_string(&self) -> Option<&str> {
        match self {
            IonValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Get as i64 if this is an Int value.
    #[inline]
    pub fn as_int(&self) -> Option<i64> {
        match self {
            IonValue::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// Get as symbol ID if this is a Symbol value.
    #[inline]
    pub fn as_symbol(&self) -> Option<u32> {
        match self {
            IonValue::Symbol(id) => Some(*id),
            _ => None,
        }
    }

    /// Get as list if this is a List value.
    #[inline]
    pub fn as_list(&self) -> Option<&[IonValue]> {
        match self {
            IonValue::List(items) => Some(items),
            _ => None,
        }
    }

    /// Get struct fields if this is a Struct value.
    #[inline]
    pub fn as_struct(&self) -> Option<&[(u32, IonValue)]> {
        match self {
            IonValue::Struct(fields) => Some(fields),
            _ => None,
        }
    }

    /// Get field from struct by symbol ID. O(n) scan - optimal for small structs.
    #[inline]
    pub fn get(&self, symbol_id: u32) -> Option<&IonValue> {
        self.as_struct()?
            .iter()
            .find(|(k, _)| *k == symbol_id)
            .map(|(_, v)| v)
    }

    /// Unwrap annotated value to get inner value.
    pub fn unwrap_annotated(&self) -> &IonValue {
        match self {
            IonValue::Annotated(_, inner) => inner.unwrap_annotated(),
            other => other,
        }
    }
}

/// Ion binary parser.
pub struct IonParser<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> IonParser<'a> {
    /// Create a new parser for the given data.
    #[inline]
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Parse Ion data starting with the BVM marker.
    pub fn parse(&mut self) -> io::Result<IonValue> {
        if self.data.len() < 4 || self.data[..4] != ION_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not Ion data (missing BVM)",
            ));
        }
        self.pos = 4;
        self.parse_value()
    }

    /// Parse a single Ion value at current position.
    fn parse_value(&mut self) -> io::Result<IonValue> {
        if self.pos >= self.data.len() {
            return Ok(IonValue::Null);
        }

        let type_byte = self.data[self.pos];
        self.pos += 1;

        let type_code = type_byte >> 4;
        let length_code = type_byte & 0x0f;

        // Null is encoded as length_code 15 for any type
        if length_code == 15 {
            return Ok(IonValue::Null);
        }

        let ion_type = match IonType::from_nibble(type_code) {
            Some(t) => t,
            None => return Ok(IonValue::Null), // Reserved type
        };

        // Get actual length
        let length = if length_code == 14 {
            self.read_varuint()? as usize
        } else {
            length_code as usize
        };

        match ion_type {
            IonType::Null => {
                // Type 0 with length > 0 is a NOP pad, skip the bytes
                self.pos += length;
                Ok(IonValue::Null)
            }

            IonType::Bool => Ok(IonValue::Bool(length_code != 0)),

            IonType::PosInt => {
                let value = self.read_uint(length)?;
                if value > i64::MAX as u64 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "positive integer too large for i64",
                    ));
                }
                Ok(IonValue::Int(value as i64))
            }

            IonType::NegInt => {
                let value = self.read_uint(length)?;
                // For negative integers, the magnitude is stored, and we negate it.
                // i64::MIN has magnitude 2^63, which fits in u64 but not as positive i64.
                if value > (i64::MAX as u64) + 1 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "negative integer too large for i64",
                    ));
                }
                // Handle i64::MIN specially (magnitude 2^63 can't be negated normally)
                if value == (i64::MAX as u64) + 1 {
                    Ok(IonValue::Int(i64::MIN))
                } else {
                    Ok(IonValue::Int(-(value as i64)))
                }
            }

            IonType::Float => {
                let value = match length {
                    0 => 0.0, // Positive zero
                    4 => {
                        let bytes: [u8; 4] = self.read_bytes(4)?.try_into().unwrap();
                        f32::from_be_bytes(bytes) as f64
                    }
                    8 => {
                        let bytes: [u8; 8] = self.read_bytes(8)?.try_into().unwrap();
                        f64::from_be_bytes(bytes)
                    }
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid float length",
                        ));
                    }
                };
                Ok(IonValue::Float(value))
            }

            IonType::Decimal | IonType::Timestamp => {
                // Skip - not used in KFX reading
                self.pos += length;
                Ok(IonValue::Null)
            }

            IonType::Symbol => {
                let symbol_id = self.read_uint(length)?;
                if symbol_id > u32::MAX as u64 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "symbol ID too large",
                    ));
                }
                Ok(IonValue::Symbol(symbol_id as u32))
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
                let mut fields = Vec::new();
                while self.pos < end {
                    let field_name = self.read_varuint()?;
                    let value = self.parse_value()?;
                    fields.push((field_name, value));
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
        }
    }

    /// Read bytes from current position.
    #[inline]
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

    /// Read a VarUInt (7 bits per byte, MSB set on last byte).
    #[inline]
    fn read_varuint(&mut self) -> io::Result<u32> {
        let mut result: u32 = 0;
        loop {
            if self.pos >= self.data.len() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "unexpected end of data",
                ));
            }
            let byte = self.data[self.pos];
            self.pos += 1;
            result = (result << 7) | (byte & 0x7f) as u32;
            if byte & 0x80 != 0 {
                return Ok(result);
            }
        }
    }

    /// Read unsigned integer (big-endian, up to 8 bytes).
    #[inline]
    fn read_uint(&mut self, len: usize) -> io::Result<u64> {
        if len == 0 {
            return Ok(0);
        }
        if len > 8 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "integer too large (> 8 bytes)",
            ));
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
        let data = [0xe0, 0x01, 0x00, 0xea, 0x11]; // true
        let mut parser = IonParser::new(&data);
        assert_eq!(parser.parse().unwrap().as_int(), None);

        let data = [0xe0, 0x01, 0x00, 0xea, 0x10]; // false
        let mut parser = IonParser::new(&data);
        if let IonValue::Bool(b) = parser.parse().unwrap() {
            assert!(!b);
        } else {
            panic!("expected bool");
        }
    }

    #[test]
    fn test_parse_int() {
        let data = [0xe0, 0x01, 0x00, 0xea, 0x21, 0x2a]; // int 42
        let mut parser = IonParser::new(&data);
        assert_eq!(parser.parse().unwrap().as_int(), Some(42));
    }

    #[test]
    fn test_parse_negative_int() {
        // -42: type 3 (NegInt), length 1, magnitude 42
        let data = [0xe0, 0x01, 0x00, 0xea, 0x31, 0x2a];
        let mut parser = IonParser::new(&data);
        assert_eq!(parser.parse().unwrap().as_int(), Some(-42));
    }

    #[test]
    fn test_parse_large_positive_int() {
        // 8-byte positive integer: 0x7FFFFFFFFFFFFFFF (i64::MAX)
        let data = [
            0xe0, 0x01, 0x00, 0xea, // BVM
            0x28, // PosInt, length 8
            0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        ];
        let mut parser = IonParser::new(&data);
        assert_eq!(parser.parse().unwrap().as_int(), Some(i64::MAX));
    }

    #[test]
    fn test_parse_large_negative_int() {
        // -2^63 (i64::MIN): magnitude is 0x8000000000000000
        let data = [
            0xe0, 0x01, 0x00, 0xea, // BVM
            0x38, // NegInt, length 8
            0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let mut parser = IonParser::new(&data);
        assert_eq!(parser.parse().unwrap().as_int(), Some(i64::MIN));
    }

    #[test]
    fn test_parse_string() {
        let data = [0xe0, 0x01, 0x00, 0xea, 0x82, b'h', b'i'];
        let mut parser = IonParser::new(&data);
        assert_eq!(parser.parse().unwrap().as_string(), Some("hi"));
    }

    #[test]
    fn test_parse_struct() {
        // struct { 10: "a", 20: 1 }
        // VarUInt encoding: value with MSB set as stop bit
        // 10 = 0x0A, with MSB = 0x8A
        // 20 = 0x14, with MSB = 0x94
        let data = [
            0xe0, 0x01, 0x00, 0xea, // BVM
            0xd6,       // struct, length 6
            0x8a,       // field 10 (VarUInt: 10 | 0x80)
            0x81, b'a', // string "a"
            0x94,       // field 20 (VarUInt: 20 | 0x80)
            0x21, 0x01, // int 1
        ];
        let mut parser = IonParser::new(&data);
        let value = parser.parse().unwrap();
        assert_eq!(value.get(10).and_then(|v| v.as_string()), Some("a"));
        assert_eq!(value.get(20).and_then(|v| v.as_int()), Some(1));
    }

    #[test]
    fn test_nop_pad_skipped() {
        // NOP pad: type 0, length 3 (skip 3 bytes), followed by int 42
        // Struct content: field1(1) + nop(4) + field2(1) + int(2) = 8 bytes
        let data = [
            0xe0, 0x01, 0x00, 0xea, // BVM
            0xd8,                   // struct, length 8
            0x81,                   // field 1
            0x03, 0xAA, 0xBB, 0xCC, // NOP pad (type 0, len 3, 3 garbage bytes)
            0x82,                   // field 2
            0x21, 0x2a,             // int 42
        ];
        let mut parser = IonParser::new(&data);
        let value = parser.parse().unwrap();
        // Field 1 gets Null (from NOP), field 2 should have value 42
        assert!(matches!(value.get(1), Some(IonValue::Null)));
        assert_eq!(value.get(2).and_then(|v| v.as_int()), Some(42));
    }

    #[test]
    fn test_float_zero() {
        // Float with length 0 is positive zero
        let data = [0xe0, 0x01, 0x00, 0xea, 0x40]; // float, length 0
        let mut parser = IonParser::new(&data);
        if let IonValue::Float(f) = parser.parse().unwrap() {
            assert_eq!(f, 0.0);
            assert!(f.is_sign_positive());
        } else {
            panic!("expected float");
        }
    }

    #[test]
    fn test_float_invalid_length() {
        // Float with invalid length (e.g., 3) should error
        let data = [0xe0, 0x01, 0x00, 0xea, 0x43, 0x00, 0x00, 0x00];
        let mut parser = IonParser::new(&data);
        assert!(parser.parse().is_err());
    }
}
