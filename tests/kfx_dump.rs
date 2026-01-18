use std::env;
use std::fs::File;
use std::io::{self, Read};
use std::collections::HashMap;

// --- COPY OF ION PARSER (simplified) ---
// This is necessary because we can't access private modules from tests easily for deep inspection

const ION_MAGIC: [u8; 4] = [0xe0, 0x01, 0x00, 0xea];

#[derive(Debug, Clone)]
enum IonValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Symbol(u64),
    String(String),
    Blob(Vec<u8>),
    List(Vec<IonValue>),
    Struct(HashMap<u64, IonValue>),
    Annotated(Vec<u64>, Box<IonValue>),
    Decimal(Vec<u8>),
}

struct IonParser<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> IonParser<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn parse(&mut self) -> io::Result<IonValue> {
        if self.remaining() < 4 { return Err(io::Error::new(io::ErrorKind::InvalidData, "short data")); }
        if self.data[self.pos..self.pos+4] != ION_MAGIC { return Err(io::Error::new(io::ErrorKind::InvalidData, "not ION")); }
        self.pos += 4;
        self.parse_value()
    }

    fn parse_value(&mut self) -> io::Result<IonValue> {
        if self.remaining() == 0 { return Ok(IonValue::Null); }
        let type_byte = self.read_u8()?;
        let ion_type = type_byte >> 4;
        let length_code = type_byte & 0x0f;
        if length_code == 15 { return Ok(IonValue::Null); }
        
        let length = if length_code == 14 { self.read_varuint()? as usize } else { length_code as usize };

        match ion_type {
            0 => Ok(IonValue::Null),
            1 => Ok(IonValue::Bool(length_code != 0)),
            2 => Ok(IonValue::Int(self.read_uint(length)? as i64)),
            3 => Ok(IonValue::Int(-(self.read_uint(length)? as i64))),
            4 => Ok(IonValue::Float(0.0)), // Skip float impl for brevity
            5 => Ok(IonValue::Decimal(self.read_bytes(length)?.to_vec())),
            7 => Ok(IonValue::Symbol(self.read_uint(length)?)),
            8 => {
                let bytes = self.read_bytes(length)?;
                Ok(IonValue::String(String::from_utf8_lossy(bytes).into_owned()))
            }
            10 => Ok(IonValue::Blob(self.read_bytes(length)?.to_vec())),
            11 | 12 => {
                let end = self.pos + length;
                let mut items = Vec::new();
                while self.pos < end { items.push(self.parse_value()?); }
                Ok(IonValue::List(items))
            }
            13 => {
                let end = self.pos + length;
                let mut fields = HashMap::new();
                while self.pos < end {
                    let key = self.read_varuint()?;
                    let val = self.parse_value()?;
                    fields.insert(key, val);
                }
                Ok(IonValue::Struct(fields))
            }
            14 => {
                let end = self.pos + length;
                let ann_len = self.read_varuint()? as usize;
                let ann_end = self.pos + ann_len;
                let mut anns = Vec::new();
                while self.pos < ann_end { anns.push(self.read_varuint()?); }
                let inner = if self.pos < end { self.parse_value()? } else { IonValue::Null };
                Ok(IonValue::Annotated(anns, Box::new(inner)))
            }
            _ => { self.pos += length; Ok(IonValue::Null) }
        }
    }

    fn remaining(&self) -> usize { self.data.len().saturating_sub(self.pos) }
    fn read_u8(&mut self) -> io::Result<u8> {
        if self.pos >= self.data.len() { return Err(io::Error::from(io::ErrorKind::UnexpectedEof)); }
        let b = self.data[self.pos]; self.pos += 1; Ok(b)
    }
    fn read_bytes(&mut self, len: usize) -> io::Result<&'a [u8]> {
        if self.pos + len > self.data.len() { return Err(io::Error::from(io::ErrorKind::UnexpectedEof)); }
        let b = &self.data[self.pos..self.pos+len]; self.pos += len; Ok(b)
    }
    fn read_varuint(&mut self) -> io::Result<u64> {
        let mut res = 0;
        for _ in 0..10 {
            let b = self.read_u8()?;
            res = (res << 7) | (b & 0x7f) as u64;
            if b & 0x80 != 0 { return Ok(res); }
        }
        Err(io::Error::new(io::ErrorKind::InvalidData, "varuint too long"))
    }
    fn read_uint(&mut self, len: usize) -> io::Result<u64> {
        let bytes = self.read_bytes(len)?;
        let mut res = 0;
        for &b in bytes { res = (res << 8) | b as u64; }
        Ok(res)
    }
}

// --- DUMP LOGIC ---

fn dump_kfx(path: &str) {
    let mut file = File::open(path).expect("File not found");
    let mut data = Vec::new();
    file.read_to_end(&mut data).expect("Read failed");

    if data.len() < 18 || &data[0..4] != b"CONT" { panic!("Not KFX"); }

    let header_len = u32::from_le_bytes(data[6..10].try_into().unwrap()) as usize;
    let mut pos = 18;

    println!("Dumping KFX: {}", path);

    while pos + 24 <= data.len() && &data[pos..pos+4] != ION_MAGIC {
        let id = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
        let ftype = u32::from_le_bytes(data[pos+4..pos+8].try_into().unwrap());
        let offset = u64::from_le_bytes(data[pos+8..pos+16].try_into().unwrap()) as usize;
        let length = u64::from_le_bytes(data[pos+16..pos+24].try_into().unwrap()) as usize;

        // Only interested in Styles ($157)
        if ftype == 157 {
            println!("\nFragment $157 (Style) ID={}", id);
            let payload_start = header_len + offset;
            let payload = &data[payload_start..payload_start+length];
            
            // Skip ENTY header (10 bytes usually) + Header ION
            if payload.len() > 10 && &payload[0..4] == b"ENTY" {
                 // The ENTY header contains an ION struct with metadata, then the actual content ION
                 // We need to find the second ION BVM
                 let ion_data = &payload[10..];
                 let mut found = false;
                 // Find second BVM
                 for i in 4..ion_data.len()-4 {
                     if &ion_data[i..i+4] == ION_MAGIC {
                         let content_ion = &ion_data[i..];
                         let mut parser = IonParser::new(content_ion);
                         match parser.parse() {
                             Ok(val) => println!("  {:?}", val),
                             Err(e) => println!("  Error parsing ION: {}", e),
                         }
                         found = true;
                         break;
                     }
                 }
                 if !found {
                     println!("  Could not find content ION");
                 }
            }
        }
        pos += 24;
    }
}

#[test]
fn inspect_kfx_styles() {
    println!("--- REFERENCE KFX ---");
    dump_kfx("tests/fixtures/epictetus.kfx");
    
    println!("\n--- GENERATED KFX ---");
    dump_kfx("/home/zach/.gemini/tmp/d2a74f6a5bc87717965512b496c0fd4bb6b6ddee37f38e5dfef6eb0de8c5a212/epictetus.kfx");
}
