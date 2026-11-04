use super::*;

const CFF: &[u8] = include_bytes!("../../../tests/fixtures/math-fonts/boko-math-cff.otf");
const GLYF: &[u8] = include_bytes!("../../../tests/fixtures/math-fonts/boko-math-glyf.ttf");

// Compare directed segments, including the implied closing edge. Parsers
// may choose different starting points on the same closed contour or emit
// an explicit closing line. Sorting preserves each segment's coordinates
// and direction while ignoring those equivalent encodings.
fn normalized_outline(path: &[f32]) -> Vec<f32> {
    let mut segments = Vec::new();
    let mut start = [0.0; 2];
    let mut position = start;
    let mut i = 0;
    while i < path.len() {
        let count = match path[i] as u8 {
            0 | 1 => 3,
            2 => 5,
            3 => 7,
            4 => 1,
            _ => panic!("invalid path opcode"),
        };
        match path[i] as u8 {
            0 => {
                start.copy_from_slice(&path[i + 1..i + 3]);
                position = start;
            }
            4 => {
                if position != start {
                    segments.push(vec![1.0, position[0], position[1], start[0], start[1]]);
                }
                position = start;
            }
            _ => {
                let mut segment = vec![path[i], position[0], position[1]];
                segment.extend_from_slice(&path[i + 1..i + count]);
                segments.push(segment);
                position.copy_from_slice(&path[i + count - 2..i + count]);
            }
        }
        i += count;
    }
    segments.sort_by(|a, b| a.partial_cmp(b).expect("finite coordinates"));
    segments
        .into_iter()
        .flatten()
        .map(|v| if v == 0.0 { 0.0 } else { v })
        .collect()
}

fn assert_baseline(data: &[u8], expected: &str, glyph_count: u16) {
    let font = MathFont::from_bytes(data.to_vec()).unwrap();
    assert_eq!(font.scaler.font().num_glyphs(), u32::from(glyph_count));
    let mut snapshot = format!("upem {}\n{:?}\n", font.units_per_em(), font.constants());
    for gid in 0..glyph_count {
        let outline = normalized_outline(&font.outline(gid).0);
        let bytes: Vec<_> = outline.iter().flat_map(|v| v.to_le_bytes()).collect();
        let hash = sha1_smol::Sha1::from(bytes).digest();
        snapshot.push_str(&format!("{gid} {:?} {hash}", font.metrics(gid)));
        for size in [0., 500., 1000., 2000., 10000.] {
            let v = font.vertical_variant(gid, size);
            let h = font.horizontal_variant(gid, size);
            snapshot.push_str(&format!(" {v}/{h}"));
        }
        snapshot.push('\n');
    }
    for c in " abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+-=()[]∑∫√→⏞αβé𝑥𝑎𝑏𝑐𝑑𝑖𝑛\u{10ffff}".chars() {
        snapshot.push_str(&format!("U+{:X} {:?}\n", c as u32, font.glyph(c)));
    }
    assert_eq!(snapshot, expected);
}

#[test]
fn cff_matches_previous_parser() {
    assert_baseline(
        CFF,
        include_str!("../../../tests/fixtures/math-fonts/boko-math-cff.otf.txt"),
        168,
    );
}

#[test]
fn truetype_matches_previous_parser() {
    assert_baseline(
        GLYF,
        include_str!("../../../tests/fixtures/math-fonts/boko-math-glyf.ttf.txt"),
        158,
    );
}

fn table_record(data: &[u8], tag: &[u8; 4]) -> usize {
    let count = u16::from_be_bytes(data[4..6].try_into().unwrap()) as usize;
    (0..count)
        .map(|i| 12 + i * 16)
        .find(|&i| &data[i..i + 4] == tag)
        .unwrap()
}

#[test]
fn rejects_missing_or_truncated_math_tables() {
    assert!(MathFont::from_bytes(Vec::new()).is_none());
    assert!(MathFont::from_bytes(b"not a font".to_vec()).is_none());
    for data in [CFF, GLYF] {
        let record = table_record(data, b"MATH");
        let mut missing = data.to_vec();
        missing[record..record + 4].copy_from_slice(b"NONE");
        assert!(MathFont::from_bytes(missing).is_none());
        let mut truncated = data.to_vec();
        truncated[record + 12..record + 16].copy_from_slice(&10_u32.to_be_bytes());
        assert!(MathFont::from_bytes(truncated).is_none());
        let mut bad_offset = data.to_vec();
        bad_offset[record + 8..record + 12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(MathFont::from_bytes(bad_offset).is_none());
    }
}

#[test]
fn optional_math_tables_fall_back() {
    for source in [CFF, GLYF] {
        let mut data = source.to_vec();
        let record = table_record(&data, b"MATH");
        let offset = u32::from_be_bytes(data[record + 8..record + 12].try_into().unwrap()) as usize;
        // Null MathGlyphInfo and MathVariants offsets; constants remain valid.
        data[offset + 6..offset + 10].fill(0);
        let font = MathFont::from_bytes(data).unwrap();
        let integral = font.glyph('∫').unwrap();
        assert_eq!(font.metrics(integral).italic_correction, 0.0);
        assert_eq!(font.vertical_variant(integral, 10000.), integral);
        let arrow = font.glyph('→').unwrap();
        assert_eq!(font.horizontal_variant(arrow, 10000.), arrow);
    }
}

#[test]
fn empty_and_missing_glyphs_are_safe() {
    for data in [CFF, GLYF] {
        let font = MathFont::from_bytes(data.to_vec()).unwrap();
        assert_eq!(font.glyph('\u{10ffff}'), None);
        let space = font.glyph(' ').unwrap();
        assert!(font.outline(space).0.is_empty());
        assert!(font.metrics(space).advance > 0.0);
        assert_eq!(font.metrics(space).max_y, 0.0);
        let missing = u16::MAX;
        assert!(font.outline(missing).0.is_empty());
        assert_eq!(font.metrics(missing).advance, 0.0);
        assert_eq!(font.metrics(missing).max_y, 0.0);
        assert_eq!(font.vertical_variant(missing, 1000.), missing);
        assert_eq!(font.horizontal_variant(missing, 1000.), missing);
    }
}

#[test]
fn outline_cache_returns_independent_paths() {
    for data in [CFF, GLYF] {
        let font = MathFont::from_bytes(data.to_vec()).unwrap();
        // Includes a composite in the TrueType fixture.
        let gid = font.glyph('é').unwrap();
        let expected = font.outline(gid);
        assert!(!expected.0.is_empty());
        let mut copy = font.outline(gid);
        copy.0.clear();
        assert_eq!(font.outline(gid), expected);
        assert_eq!(font.outline_cache.borrow().len(), 1);
    }
}
