//! Amazon Ion binary format parser.
//!
//! Ion is Amazon's data serialization format used in KFX ebooks.
//! This implements a minimal parser sufficient for reading KFX content.
//!
//! Reference: <https://amazon-ion.github.io/ion-docs/docs/binary.html>

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
    /// Raw decimal bytes (type code 0x5X will be computed from length)
    Decimal(Vec<u8>),
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
        if self.data[self.pos..self.pos + 4] != ION_MAGIC {
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

/// Ion binary writer
pub struct IonWriter {
    buffer: Vec<u8>,
}

impl IonWriter {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Get the written data
    pub fn into_bytes(self) -> Vec<u8> {
        self.buffer
    }

    /// Write the Ion BVM (Binary Version Marker)
    pub fn write_bvm(&mut self) {
        self.buffer.extend_from_slice(&ION_MAGIC);
    }

    /// Write an IonValue
    pub fn write_value(&mut self, value: &IonValue) {
        match value {
            IonValue::Null => self.write_null(),
            IonValue::Bool(b) => self.write_bool(*b),
            IonValue::Int(n) => self.write_int(*n),
            IonValue::Float(f) => self.write_float(*f),
            IonValue::Symbol(id) => self.write_symbol(*id),
            IonValue::String(s) => self.write_string(s),
            IonValue::Blob(data) => self.write_blob(data),
            IonValue::List(items) => self.write_list(items),
            IonValue::Struct(fields) => self.write_struct(fields),
            IonValue::Annotated(annotations, inner) => self.write_annotated(annotations, inner),
            IonValue::Decimal(bytes) => self.write_decimal(bytes),
        }
    }

    /// Write decimal value (raw bytes)
    pub fn write_decimal(&mut self, bytes: &[u8]) {
        let len = bytes.len();
        if len <= 13 {
            self.buffer.push(0x50 | len as u8);
        } else {
            self.buffer.push(0x5E);
            self.write_varuint(len as u64);
        }
        self.buffer.extend_from_slice(bytes);
    }

    /// Write null value
    pub fn write_null(&mut self) {
        self.buffer.push(0x0f); // type 0, length 15 = null
    }

    /// Write boolean value
    pub fn write_bool(&mut self, value: bool) {
        self.buffer.push(if value { 0x11 } else { 0x10 });
    }

    /// Write integer value
    pub fn write_int(&mut self, value: i64) {
        if value == 0 {
            self.buffer.push(0x20); // type 2, length 0
            return;
        }

        let (type_code, magnitude) = if value >= 0 {
            (2u8, value as u64)
        } else {
            (3u8, (-value) as u64)
        };

        let bytes = uint_bytes(magnitude);
        self.write_type_descriptor(type_code, bytes.len());
        self.buffer.extend_from_slice(&bytes);
    }

    /// Write float value (always as 64-bit)
    pub fn write_float(&mut self, value: f64) {
        self.buffer.push(0x48); // type 4, length 8
        self.buffer.extend_from_slice(&value.to_be_bytes());
    }

    /// Write symbol (by ID)
    pub fn write_symbol(&mut self, id: u64) {
        if id == 0 {
            self.buffer.push(0x70); // type 7, length 0
            return;
        }
        let bytes = uint_bytes(id);
        self.write_type_descriptor(7, bytes.len());
        self.buffer.extend_from_slice(&bytes);
    }

    /// Write string value
    pub fn write_string(&mut self, s: &str) {
        let bytes = s.as_bytes();
        self.write_type_descriptor(8, bytes.len());
        self.buffer.extend_from_slice(bytes);
    }

    /// Write blob value
    pub fn write_blob(&mut self, data: &[u8]) {
        self.write_type_descriptor(10, data.len());
        self.buffer.extend_from_slice(data);
    }

    /// Write list value
    pub fn write_list(&mut self, items: &[IonValue]) {
        // First, serialize items to temp buffer to get length
        let mut inner = IonWriter::new();
        for item in items {
            inner.write_value(item);
        }
        let inner_bytes = inner.into_bytes();

        self.write_type_descriptor(11, inner_bytes.len());
        self.buffer.extend_from_slice(&inner_bytes);
    }

    /// Write struct value (symbol ID keys)
    pub fn write_struct(&mut self, fields: &HashMap<u64, IonValue>) {
        // Serialize fields to temp buffer
        let mut inner = IonWriter::new();

        // Sort keys in ascending order (matches Kindle reference files)
        let mut keys: Vec<_> = fields.keys().collect();
        keys.sort();

        for &key in &keys {
            inner.write_varuint(*key);
            inner.write_value(&fields[key]);
        }
        let inner_bytes = inner.into_bytes();

        self.write_type_descriptor(13, inner_bytes.len());
        self.buffer.extend_from_slice(&inner_bytes);
    }

    /// Write annotated value
    pub fn write_annotated(&mut self, annotations: &[u64], inner: &IonValue) {
        // Serialize annotation IDs
        let mut ann_buf = Vec::new();
        for &ann in annotations {
            write_varuint_to(&mut ann_buf, ann);
        }

        // Serialize inner value
        let mut inner_writer = IonWriter::new();
        inner_writer.write_value(inner);
        let inner_bytes = inner_writer.into_bytes();

        // Total length = annot_length varuint + annotations + inner value
        let mut content = Vec::new();
        write_varuint_to(&mut content, ann_buf.len() as u64);
        content.extend_from_slice(&ann_buf);
        content.extend_from_slice(&inner_bytes);

        self.write_type_descriptor(14, content.len());
        self.buffer.extend_from_slice(&content);
    }

    /// Write type descriptor byte(s)
    fn write_type_descriptor(&mut self, type_code: u8, length: usize) {
        if length < 14 {
            self.buffer.push((type_code << 4) | (length as u8));
        } else {
            self.buffer.push((type_code << 4) | 14);
            self.write_varuint(length as u64);
        }
    }

    /// Write VarUInt
    fn write_varuint(&mut self, value: u64) {
        write_varuint_to(&mut self.buffer, value);
    }
}

impl Default for IonWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Encode unsigned int as big-endian bytes (minimal length)
fn uint_bytes(value: u64) -> Vec<u8> {
    if value == 0 {
        return vec![];
    }
    let bytes = value.to_be_bytes();
    let skip = bytes.iter().take_while(|&&b| b == 0).count();
    bytes[skip..].to_vec()
}

/// Write VarUInt to a buffer (7 bits per byte, MSB set on last byte)
fn write_varuint_to(buf: &mut Vec<u8>, value: u64) {
    if value == 0 {
        buf.push(0x80);
        return;
    }

    // Count how many 7-bit groups we need
    let mut temp = value;
    let mut groups = Vec::new();
    while temp > 0 {
        groups.push((temp & 0x7f) as u8);
        temp >>= 7;
    }

    // Write in reverse order, setting MSB on last byte
    for (i, &group) in groups.iter().rev().enumerate() {
        if i == groups.len() - 1 {
            buf.push(group | 0x80); // Last byte has MSB set
        } else {
            buf.push(group);
        }
    }
}

/// Encode a float as a KFX/Ion decimal (exponent + coefficient)
/// Uses a fixed precision of 2 decimal places (e.g., 1.25 -> 125 * 10^-2)
pub(crate) fn encode_kfx_decimal(val: f32) -> Vec<u8> {
    if val == 0.0 {
        return vec![0x80]; // Exponent 0, Coef 0
    }

    // Convert to coefficient and exponent
    // Start with precision 2
    let mut coef = (val * 100.0).round() as i64;
    let mut exp: i32 = -2;

    // Normalize: remove trailing zeros
    while coef != 0 && coef % 10 == 0 {
        coef /= 10;
        exp += 1;
    }

    let mut bytes = Vec::new();

    // 1. Encode Exponent (VarInt Signed)
    // - Magnitude = abs(val)
    // - Sign bit = 0x40 (bit 6) if negative
    // - Stop bit = 0x80 (bit 7)
    // This simplified encoding assumes exponent fits in 6 bits (-63 to 63)
    let exp_mag = exp.abs();
    let exp_sign = if exp < 0 { 0x40 } else { 0x00 };
    bytes.push(0x80 | exp_sign | (exp_mag as u8 & 0x3F));

    // 2. Encode Coefficient (Int Signed)
    if coef != 0 {
        let coef_mag = coef.unsigned_abs();
        let is_neg = coef < 0;

        // Serialize magnitude (Big Endian)
        let mut mag_bytes = Vec::new();
        let mut temp = coef_mag;
        while temp > 0 {
            mag_bytes.push((temp & 0xFF) as u8);
            temp >>= 8;
        }
        if mag_bytes.is_empty() {
            mag_bytes.push(0);
        }
        mag_bytes.reverse();

        // Handle sign bit (MSB of first byte)
        // If MSB is already set by magnitude, or if we need to set it for negative
        if (mag_bytes[0] & 0x80) != 0 {
            // Padding needed
            let padding = if is_neg { 0x80 } else { 0x00 };
            bytes.push(padding);
            bytes.extend(mag_bytes);
        } else {
            if is_neg {
                mag_bytes[0] |= 0x80;
            }
            bytes.extend(mag_bytes);
        }
    }

    bytes
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

    #[test]
    fn test_write_bool_roundtrip() {
        let mut writer = IonWriter::new();
        writer.write_bvm();
        writer.write_bool(true);
        let data = writer.into_bytes();

        let mut parser = IonParser::new(&data);
        let value = parser.parse().unwrap();
        assert_eq!(value.as_bool(), Some(true));
    }

    #[test]
    fn test_write_int_roundtrip() {
        let mut writer = IonWriter::new();
        writer.write_bvm();
        writer.write_int(12345);
        let data = writer.into_bytes();

        let mut parser = IonParser::new(&data);
        let value = parser.parse().unwrap();
        assert_eq!(value.as_int(), Some(12345));
    }

    #[test]
    fn test_write_string_roundtrip() {
        let mut writer = IonWriter::new();
        writer.write_bvm();
        writer.write_string("hello world");
        let data = writer.into_bytes();

        let mut parser = IonParser::new(&data);
        let value = parser.parse().unwrap();
        assert_eq!(value.as_string(), Some("hello world"));
    }

    #[test]
    fn test_write_list_roundtrip() {
        let list = IonValue::List(vec![
            IonValue::String("item1".to_string()),
            IonValue::Int(42),
            IonValue::Bool(true),
        ]);

        let mut writer = IonWriter::new();
        writer.write_bvm();
        writer.write_value(&list);
        let data = writer.into_bytes();

        let mut parser = IonParser::new(&data);
        let value = parser.parse().unwrap();
        let items = value.as_list().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].as_string(), Some("item1"));
        assert_eq!(items[1].as_int(), Some(42));
        assert_eq!(items[2].as_bool(), Some(true));
    }

    #[test]
    fn test_write_struct_roundtrip() {
        let mut fields = HashMap::new();
        fields.insert(100, IonValue::String("value1".to_string()));
        fields.insert(200, IonValue::Int(999));
        let structure = IonValue::Struct(fields);

        let mut writer = IonWriter::new();
        writer.write_bvm();
        writer.write_value(&structure);
        let data = writer.into_bytes();

        let mut parser = IonParser::new(&data);
        let value = parser.parse().unwrap();
        let map = value.as_struct().unwrap();
        assert_eq!(map.get(&100).and_then(|v| v.as_string()), Some("value1"));
        assert_eq!(map.get(&200).and_then(|v| v.as_int()), Some(999));
    }

    #[test]
    #[ignore] // requires reference file
    fn test_parse_entity_259() {
        use std::fs;

        // Parse reference file
        let data = match fs::read("/tmp/krdsrw/tests/the-tempest.kfx") {
            Ok(d) => d,
            Err(_) => return, // Skip if file not available
        };

        let header_len = u32::from_le_bytes(data[6..10].try_into().unwrap()) as usize;
        let mut pos = 18;

        while pos + 24 <= data.len() && data[pos..pos + 4] != ION_MAGIC {
            let id = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
            let etype = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap());
            let offset = u64::from_le_bytes(data[pos + 8..pos + 16].try_into().unwrap()) as usize;
            let length = u64::from_le_bytes(data[pos + 16..pos + 24].try_into().unwrap()) as usize;

            // Show format capabilities (585)
            if etype == 585 {
                println!("Entity type={} id={}", etype, id);
                let payload_start = header_len + offset;
                let payload = &data[payload_start..payload_start + length];

                if payload.len() > 14 && &payload[0..4] == b"ENTY" {
                    // Parse the ION data after ENTY header (10 bytes)
                    let ion_data = &payload[10..];
                    let mut parser = IonParser::new(ion_data);

                    // First ION document is small header
                    if let Ok(header) = parser.parse() {
                        println!(
                            "Entity 259 id={} header: {:?}",
                            id,
                            summarize_value(&header, 0)
                        );
                    }

                    // Find and parse second ION document (starts after first BVM + header struct)
                    // Look for next ION_MAGIC in the data
                    let mut found_second = false;
                    for i in 4..ion_data.len().saturating_sub(4) {
                        if ion_data[i..i + 4] == ION_MAGIC {
                            let second_doc = &ion_data[i..];
                            let mut parser2 = IonParser::new(second_doc);
                            if let Ok(content) = parser2.parse() {
                                println!("  content: {:?}", summarize_value(&content, 0));
                                found_second = true;
                            }
                            break;
                        }
                    }
                    if !found_second {
                        println!("  (no second ION document)");
                    }
                }
            }
            pos += 24;
        }
    }

    fn summarize_value(value: &IonValue, depth: usize) -> String {
        if depth > 10 {
            return "...".to_string();
        }
        match value {
            IonValue::Null => "null".to_string(),
            IonValue::Bool(b) => format!("{}", b),
            IonValue::Int(i) => format!("{}", i),
            IonValue::Float(f) => format!("{}", f),
            IonValue::Symbol(s) => format!("${}", s),
            IonValue::String(s) => {
                // Safe truncation at char boundary
                let display = if s.chars().count() > 30 {
                    let truncated: String = s.chars().take(30).collect();
                    format!("{}...", truncated)
                } else {
                    s.clone()
                };
                format!("{:?}", display)
            }
            IonValue::Blob(b) => format!("blob[{}]", b.len()),
            IonValue::List(items) => {
                let inner: Vec<_> = items
                    .iter()
                    .take(3)
                    .map(|v| summarize_value(v, depth + 1))
                    .collect();
                if items.len() > 3 {
                    format!("[{}, ... +{}]", inner.join(", "), items.len() - 3)
                } else {
                    format!("[{}]", inner.join(", "))
                }
            }
            IonValue::Struct(fields) => {
                let inner: Vec<_> = fields
                    .iter()
                    .take(15)
                    .map(|(k, v)| format!("${}:{}", k, summarize_value(v, depth + 1)))
                    .collect();
                if fields.len() > 15 {
                    format!("{{{}, ... +{}}}", inner.join(", "), fields.len() - 15)
                } else {
                    format!("{{{}}}", inner.join(", "))
                }
            }
            IonValue::Annotated(annotations, value) => {
                let ann: Vec<_> = annotations.iter().map(|a| format!("${}", a)).collect();
                format!("{}::{}", ann.join("::"), summarize_value(value, depth))
            }
            IonValue::Decimal(bytes) => {
                let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
                format!("decimal:{}", hex)
            }
        }
    }
}
