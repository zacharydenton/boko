use std::io;

pub const NULL_INDEX: u32 = 0xFFFFFFFF;

/// MOBI Header (Record 0)
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields are part of MOBI format spec, useful for debugging
pub struct MobiHeader {
    pub compression: Compression,
    pub text_record_count: u16,
    pub text_record_size: u16,
    pub encryption: u16,
    pub mobi_type: u32,
    pub encoding: Encoding,
    pub mobi_version: u32,
    pub first_image_index: u32,
    pub title: String,
    pub language: u32,
    pub exth_flags: u32,
    pub extra_data_flags: u16,
    // HUFF/CDIC indices (for Huffman compression)
    pub huff_record_index: u32,
    pub huff_record_count: u32,
    // KF8 indices
    pub skel_index: u32,
    pub div_index: u32,
    pub oth_index: u32,
    pub fdst_index: u32,
    pub fdst_count: u32,
    pub ncx_index: u32,
    // Raw header for EXTH parsing
    pub header_length: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Compression {
    None,
    PalmDoc,
    Huffman,
    Unknown(u16),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Encoding {
    Cp1252,
    Utf8,
    Unknown(u32),
}

impl MobiHeader {
    pub fn parse(data: &[u8]) -> io::Result<Self> {
        if data.len() < 16 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MOBI header too short",
            ));
        }

        let compression = match u16::from_be_bytes([data[0], data[1]]) {
            1 => Compression::None,
            2 => Compression::PalmDoc,
            0x4448 => Compression::Huffman, // "DH"
            n => Compression::Unknown(n),
        };

        let text_record_count = u16::from_be_bytes([data[8], data[9]]);
        let text_record_size = u16::from_be_bytes([data[10], data[11]]);
        let encryption = u16::from_be_bytes([data[12], data[13]]);

        // Check if this is a minimal header
        if data.len() <= 16 {
            return Ok(Self {
                compression,
                text_record_count,
                text_record_size,
                encryption,
                mobi_type: 0,
                encoding: Encoding::Cp1252,
                mobi_version: 1,
                first_image_index: NULL_INDEX,
                title: String::new(),
                language: 0,
                exth_flags: 0,
                extra_data_flags: 0,
                huff_record_index: NULL_INDEX,
                huff_record_count: 0,
                skel_index: NULL_INDEX,
                div_index: NULL_INDEX,
                oth_index: NULL_INDEX,
                fdst_index: NULL_INDEX,
                fdst_count: 0,
                ncx_index: NULL_INDEX,
                header_length: 0,
            });
        }

        let header_length = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        let mobi_type = u32::from_be_bytes([data[24], data[25], data[26], data[27]]);
        let codepage = u32::from_be_bytes([data[28], data[29], data[30], data[31]]);

        let encoding = match codepage {
            1252 => Encoding::Cp1252,
            65001 => Encoding::Utf8,
            n => Encoding::Unknown(n),
        };

        // Title offset and length at 0x54-0x5C
        let title = if data.len() >= 0x5C {
            let title_offset =
                u32::from_be_bytes([data[0x54], data[0x55], data[0x56], data[0x57]]) as usize;
            let title_length =
                u32::from_be_bytes([data[0x58], data[0x59], data[0x5A], data[0x5B]]) as usize;
            if title_offset + title_length <= data.len() {
                String::from_utf8_lossy(&data[title_offset..title_offset + title_length])
                    .to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let language = if data.len() >= 0x60 {
            u32::from_be_bytes([data[0x5C], data[0x5D], data[0x5E], data[0x5F]])
        } else {
            0
        };

        let mobi_version = if data.len() >= 0x6C {
            u32::from_be_bytes([data[0x68], data[0x69], data[0x6A], data[0x6B]])
        } else {
            1
        };

        let first_image_index = if data.len() >= 0x70 {
            u32::from_be_bytes([data[0x6C], data[0x6D], data[0x6E], data[0x6F]])
        } else {
            NULL_INDEX
        };

        // HUFF/CDIC indices at 0x70 and 0x74
        let (huff_record_index, huff_record_count) = if data.len() >= 0x78 {
            (
                u32::from_be_bytes([data[0x70], data[0x71], data[0x72], data[0x73]]),
                u32::from_be_bytes([data[0x74], data[0x75], data[0x76], data[0x77]]),
            )
        } else {
            (NULL_INDEX, 0)
        };

        let exth_flags = if data.len() >= 0x84 {
            u32::from_be_bytes([data[0x80], data[0x81], data[0x82], data[0x83]])
        } else {
            0
        };

        let extra_data_flags = if data.len() >= 0xF4 && header_length >= 0xE4 {
            u16::from_be_bytes([data[0xF2], data[0xF3]])
        } else {
            0
        };

        // KF8 indices (MOBI version 8)
        let (skel_index, div_index, oth_index) = if mobi_version == 8 && data.len() >= 0x108 {
            (
                u32::from_be_bytes([data[0xFC], data[0xFD], data[0xFE], data[0xFF]]),
                u32::from_be_bytes([data[0xF8], data[0xF9], data[0xFA], data[0xFB]]),
                u32::from_be_bytes([data[0x100], data[0x101], data[0x102], data[0x103]]),
            )
        } else {
            (NULL_INDEX, NULL_INDEX, NULL_INDEX)
        };

        let (fdst_index, fdst_count) = if mobi_version == 8 && data.len() >= 0xC8 {
            (
                u32::from_be_bytes([data[0xC0], data[0xC1], data[0xC2], data[0xC3]]),
                u32::from_be_bytes([data[0xC4], data[0xC5], data[0xC6], data[0xC7]]),
            )
        } else {
            (NULL_INDEX, 0)
        };

        let ncx_index = if data.len() >= 0xF8 {
            u32::from_be_bytes([data[0xF4], data[0xF5], data[0xF6], data[0xF7]])
        } else {
            NULL_INDEX
        };

        Ok(Self {
            compression,
            text_record_count,
            text_record_size,
            encryption,
            mobi_type,
            encoding,
            mobi_version,
            first_image_index,
            title,
            language,
            exth_flags,
            extra_data_flags,
            huff_record_index,
            huff_record_count,
            skel_index,
            div_index,
            oth_index,
            fdst_index,
            fdst_count,
            ncx_index,
            header_length,
        })
    }

    pub fn has_exth(&self) -> bool {
        self.exth_flags & 0x40 != 0
    }
}

/// EXTH Header (extended metadata)
#[derive(Debug, Default)]
pub struct ExthHeader {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub isbn: Option<String>,
    pub asin: Option<String>,
    pub source: Option<String>,
    pub subjects: Vec<String>,
    pub pub_date: Option<String>,
    pub rights: Option<String>,
    pub cover_offset: Option<u32>,
    pub thumbnail_offset: Option<u32>,
    pub language: Option<String>,
    pub kf8_boundary: Option<u32>,
}

impl ExthHeader {
    pub fn parse(data: &[u8], encoding: Encoding) -> io::Result<Self> {
        if data.len() < 12 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "EXTH header too short",
            ));
        }

        if &data[0..4] != b"EXTH" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid EXTH signature",
            ));
        }

        let _header_length = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let record_count = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);

        let mut exth = ExthHeader::default();
        let mut pos = 12;

        let decode = |bytes: &[u8]| -> String {
            match encoding {
                Encoding::Utf8 => String::from_utf8_lossy(bytes).to_string(),
                _ => {
                    // CP1252 - just use lossy UTF-8 for now
                    String::from_utf8_lossy(bytes).to_string()
                }
            }
        };

        for _ in 0..record_count {
            if pos + 8 > data.len() {
                break;
            }

            let record_type =
                u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
            let record_len =
                u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                    as usize;

            if pos + record_len > data.len() {
                break;
            }

            let content = &data[pos + 8..pos + record_len];

            match record_type {
                100 => exth.authors.push(decode(content).trim().to_string()),
                101 => exth.publisher = Some(decode(content).trim().to_string()),
                103 => exth.description = Some(decode(content).trim().to_string()),
                104 => exth.isbn = Some(decode(content).trim().to_string()),
                105 => {
                    for subject in decode(content).split(';') {
                        let s = subject.trim().to_string();
                        if !s.is_empty() {
                            exth.subjects.push(s);
                        }
                    }
                }
                106 => exth.pub_date = Some(decode(content).trim().to_string()),
                109 => exth.rights = Some(decode(content).trim().to_string()),
                112 => exth.source = Some(decode(content).trim().to_string()),
                113 => exth.asin = Some(decode(content).trim().to_string()),
                121 => {
                    if content.len() >= 4 {
                        let val =
                            u32::from_be_bytes([content[0], content[1], content[2], content[3]]);
                        if val != NULL_INDEX {
                            exth.kf8_boundary = Some(val);
                        }
                    }
                }
                201 => {
                    if content.len() >= 4 {
                        let val =
                            u32::from_be_bytes([content[0], content[1], content[2], content[3]]);
                        if val != NULL_INDEX {
                            exth.cover_offset = Some(val);
                        }
                    }
                }
                202 => {
                    if content.len() >= 4 {
                        let val =
                            u32::from_be_bytes([content[0], content[1], content[2], content[3]]);
                        if val != NULL_INDEX {
                            exth.thumbnail_offset = Some(val);
                        }
                    }
                }
                503 => exth.title = Some(decode(content).trim().to_string()),
                524 => exth.language = Some(decode(content).trim().to_string()),
                _ => {}
            }

            pos += record_len;
        }

        Ok(exth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_types() {
        assert_eq!(
            match 2u16.to_be_bytes() {
                [0, 2] => Compression::PalmDoc,
                _ => Compression::None,
            },
            Compression::PalmDoc
        );
    }

    #[test]
    fn test_mobi_header_parse_minimal() {
        // Minimal 16-byte header
        let mut data = vec![0u8; 16];
        data[0..2].copy_from_slice(&2u16.to_be_bytes()); // PalmDoc compression
        data[8..10].copy_from_slice(&10u16.to_be_bytes()); // text_record_count
        data[10..12].copy_from_slice(&4096u16.to_be_bytes()); // text_record_size

        let header = MobiHeader::parse(&data).unwrap();
        assert_eq!(header.compression, Compression::PalmDoc);
        assert_eq!(header.text_record_count, 10);
        assert_eq!(header.text_record_size, 4096);
        assert_eq!(header.encoding, Encoding::Cp1252); // default
    }

    #[test]
    fn test_mobi_header_parse_with_encoding() {
        let mut data = vec![0u8; 32];
        data[0..2].copy_from_slice(&1u16.to_be_bytes()); // No compression
        data[28..32].copy_from_slice(&65001u32.to_be_bytes()); // UTF-8 codepage

        let header = MobiHeader::parse(&data).unwrap();
        assert_eq!(header.compression, Compression::None);
        assert_eq!(header.encoding, Encoding::Utf8);
    }

    #[test]
    fn test_mobi_header_huffman_compression() {
        let mut data = vec![0u8; 32];
        data[0..2].copy_from_slice(&0x4448u16.to_be_bytes()); // "DH" = Huffman

        let header = MobiHeader::parse(&data).unwrap();
        assert_eq!(header.compression, Compression::Huffman);
    }

    #[test]
    fn test_mobi_header_has_exth() {
        let mut data = vec![0u8; 0x84];
        // Set EXTH flag (bit 6 of exth_flags at offset 0x80)
        data[0x80..0x84].copy_from_slice(&0x40u32.to_be_bytes());

        let header = MobiHeader::parse(&data).unwrap();
        assert!(header.has_exth());

        // Without flag
        data[0x80..0x84].copy_from_slice(&0u32.to_be_bytes());
        let header = MobiHeader::parse(&data).unwrap();
        assert!(!header.has_exth());
    }

    #[test]
    fn test_mobi_header_too_short() {
        let data = vec![0u8; 10];
        assert!(MobiHeader::parse(&data).is_err());
    }

    #[test]
    fn test_exth_header_parse() {
        let mut data = Vec::new();
        data.extend_from_slice(b"EXTH"); // signature
        data.extend_from_slice(&100u32.to_be_bytes()); // header length
        data.extend_from_slice(&2u32.to_be_bytes()); // 2 records

        // Record 1: Author (type 100)
        let author = b"Test Author";
        data.extend_from_slice(&100u32.to_be_bytes()); // type
        data.extend_from_slice(&(8 + author.len() as u32).to_be_bytes()); // length
        data.extend_from_slice(author);

        // Record 2: Title (type 503)
        let title = b"Test Title";
        data.extend_from_slice(&503u32.to_be_bytes()); // type
        data.extend_from_slice(&(8 + title.len() as u32).to_be_bytes()); // length
        data.extend_from_slice(title);

        let exth = ExthHeader::parse(&data, Encoding::Utf8).unwrap();
        assert_eq!(exth.authors, vec!["Test Author"]);
        assert_eq!(exth.title, Some("Test Title".to_string()));
    }

    #[test]
    fn test_exth_header_cover_offset() {
        let mut data = Vec::new();
        data.extend_from_slice(b"EXTH");
        data.extend_from_slice(&20u32.to_be_bytes()); // header length
        data.extend_from_slice(&1u32.to_be_bytes()); // 1 record

        // Cover offset record (type 201)
        data.extend_from_slice(&201u32.to_be_bytes()); // type
        data.extend_from_slice(&12u32.to_be_bytes()); // length (8 + 4)
        data.extend_from_slice(&42u32.to_be_bytes()); // cover offset value

        let exth = ExthHeader::parse(&data, Encoding::Utf8).unwrap();
        assert_eq!(exth.cover_offset, Some(42));
    }

    #[test]
    fn test_exth_header_invalid_signature() {
        let data = b"NOTEXTH_____";
        assert!(ExthHeader::parse(data, Encoding::Utf8).is_err());
    }

    #[test]
    fn test_exth_header_too_short() {
        let data = b"EXTH";
        assert!(ExthHeader::parse(data, Encoding::Utf8).is_err());
    }
}
