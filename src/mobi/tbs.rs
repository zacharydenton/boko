//! Trailing Byte Sequence (TBS) generation for KF8 text records.
//!
//! Every KF8 text record carries a trailing byte sequence describing which
//! NCX entries start, complete, span, or end inside that record. Kindle uses
//! these to build its position map; without them, modern firmware refuses to
//! open the book ("Unable to Open Item").
//!
//! This is a faithful Rust port of calibre's `mobi/writer8/tbs.py` and the
//! supporting encoders in `mobi/utils.py` (`encode_tbs`, `encode_fvwi`,
//! `encode_trailing_data`, backward `encint`).

use std::collections::BTreeMap;

/// Forward variable-width integer (high bit set on the LAST byte of the
/// resulting bytestring — i.e. the byte that terminates the run).
fn encint_forward(value: u64) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::new();
    let mut v = value;
    loop {
        bytes.push((v & 0x7F) as u8);
        v >>= 7;
        if v == 0 {
            break;
        }
    }
    // High bit on the first appended byte (becomes last after reverse).
    if let Some(first) = bytes.first_mut() {
        *first |= 0x80;
    }
    bytes.reverse();
    bytes
}

/// Backward variable-width integer (high bit set on the FIRST byte of the
/// resulting bytestring). Used for trailing-data size suffixes — readers walk
/// backwards from end of record until they hit the high-bit byte.
fn encint_backward(value: u64) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::new();
    let mut v = value;
    loop {
        bytes.push((v & 0x7F) as u8);
        v >>= 7;
        if v == 0 {
            break;
        }
    }
    // High bit on the LAST appended byte (becomes first after reverse).
    if let Some(last) = bytes.last_mut() {
        *last |= 0x80;
    }
    bytes.reverse();
    bytes
}

/// Encode `value` shifted left by `flag_size` bits, OR'd with the low
/// `flag_size` bits of `flags`, as a forward vwi.
fn encode_fvwi(val: u64, flags: u64, flag_size: u32) -> Vec<u8> {
    let mask = (1u64 << flag_size) - 1;
    let combined = (val << flag_size) | (flags & mask);
    encint_forward(combined)
}

/// A single TBS sequence: (value, flag bits → optional payload).
///
/// Flag bits per calibre:
///   0b0001 → followed by extra varint
///   0b0010 → followed by extra varint (tbs_type)
///   0b0100 → followed by single byte (sibling count)
///   0b1000 → boolean (used for cross-strand indexing, no payload)
#[derive(Debug, Default, Clone)]
struct Extras {
    bit0001: Option<u64>,
    bit0010: Option<u64>,
    bit0100: Option<u8>,
    bit1000: bool,
}

impl Extras {
    fn flags(&self) -> u64 {
        let mut f = 0;
        if self.bit0001.is_some() {
            f |= 0b0001;
        }
        if self.bit0010.is_some() {
            f |= 0b0010;
        }
        if self.bit0100.is_some() {
            f |= 0b0100;
        }
        if self.bit1000 {
            f |= 0b1000;
        }
        f
    }
}

/// Encode one (val, extras) pair as TBS bytes.
fn encode_tbs(val: u64, extras: &Extras, flag_size: u32) -> Vec<u8> {
    let mut out = encode_fvwi(val, extras.flags(), flag_size);
    if let Some(v) = extras.bit0010 {
        out.extend(encint_forward(v));
    }
    if let Some(b) = extras.bit0100 {
        out.push(b);
    }
    if let Some(v) = extras.bit0001 {
        out.extend(encint_forward(v));
    }
    out
}

/// Wrap `raw` with a backward varint size suffix so the trailer is
/// self-describing: `<raw><size>` where size = len(raw) + len(size_encoding).
pub fn encode_trailing_data(raw: &[u8]) -> Vec<u8> {
    let mut lsize: usize = 1;
    let encoded = loop {
        let candidate = encint_backward((raw.len() + lsize) as u64);
        if candidate.len() == lsize {
            break candidate;
        }
        lsize += 1;
    };
    let mut out = Vec::with_capacity(raw.len() + encoded.len());
    out.extend_from_slice(raw);
    out.extend(encoded);
    out
}

// ---------------------------------------------------------------------------
// Index entry / strand machinery
// ---------------------------------------------------------------------------

/// One NCX entry plus its action for a specific text record. `start_offset`
/// and `length_offset` from calibre's tuple aren't used after the action is
/// computed in our simplified encoder, so they're dropped here.
#[derive(Debug, Clone)]
struct LocalEntry {
    index: u32,
    depth: u32,
    parent: i32,
    action: Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Starts,
    Completes,
    Spans,
    Ends,
}

/// NCX entry as seen by the TBS computation. `start`/`length` are byte
/// positions inside the text flow.
#[derive(Debug, Clone)]
pub struct TbsEntry {
    pub index: u32,
    pub start: u64,
    pub length: u64,
    pub depth: u32,
    pub parent: i32,
}

fn fill_entry(entry: &TbsEntry, record_start: u64, record_length: u64) -> LocalEntry {
    let start_offset = entry.start as i64 - record_start as i64;
    let length_offset = start_offset + entry.length as i64;
    let rec_len = record_length as i64;
    let action = if start_offset < 0 {
        if length_offset > rec_len {
            Action::Spans
        } else {
            Action::Ends
        }
    } else if length_offset > rec_len {
        Action::Starts
    } else {
        Action::Completes
    };
    LocalEntry {
        index: entry.index,
        depth: entry.depth,
        parent: entry.parent,
        action,
    }
}

/// Mirrors calibre `populate_strand`: depth-first add the first child, then
/// gather contiguous-index siblings without children of their own.
fn populate_strand(parent: LocalEntry, entries: &mut Vec<LocalEntry>) -> Vec<LocalEntry> {
    let mut ans = vec![parent.clone()];

    let first_child_pos = entries.iter().position(|c| c.parent == parent.index as i32);
    if let Some(pos) = first_child_pos {
        let child = entries.remove(pos);
        ans.extend(populate_strand(child, entries));
    } else {
        // Contiguous-index siblings with the same parent and depth that
        // themselves have no children may share a strand layer (the 0b100
        // flag).
        let mut current_index = parent.index;
        let mut siblings: Vec<LocalEntry> = Vec::new();
        let mut i = 0;
        while i < entries.len() {
            let entry = &entries[i];
            if entry.depth == parent.depth
                && entry.parent == parent.parent
                && entry.index == current_index + 1
            {
                current_index += 1;
                let entry = entries.remove(i);
                let has_children = entries.iter().any(|c| c.parent == entry.index as i32);
                if has_children {
                    siblings.extend(populate_strand(entry, entries));
                    break;
                } else {
                    siblings.push(entry);
                    // Don't advance i — we removed at i.
                }
            } else {
                i += 1;
            }
        }
        ans.extend(siblings);
    }
    ans
}

/// Split a record's entries into strands. Each strand is grouped by depth.
fn separate_strands(mut entries: Vec<LocalEntry>) -> Vec<BTreeMap<u32, Vec<LocalEntry>>> {
    let mut ans = Vec::new();
    while !entries.is_empty() {
        let top = entries.remove(0);
        let strand = populate_strand(top, &mut entries);
        let mut layers: BTreeMap<u32, Vec<LocalEntry>> = BTreeMap::new();
        for entry in strand {
            layers.entry(entry.depth).or_default().push(entry);
        }
        ans.push(layers);
    }
    ans
}

#[derive(Debug)]
struct NegativeStrandIndex;

fn encode_strands_as_sequences(
    strands: &[BTreeMap<u32, Vec<LocalEntry>>],
    tbs_type: u64,
) -> Result<Vec<(u64, Extras)>, NegativeStrandIndex> {
    let mut ans: Vec<(u64, Extras)> = Vec::new();
    let mut last_index: Option<u32> = None;

    // Find the very first entry across all strands/layers (deterministic).
    let mut first_entry_index: Option<u32> = None;
    'outer: for strand in strands {
        for entries in strand.values() {
            if let Some(first) = entries.first() {
                first_entry_index = Some(first.index);
                break 'outer;
            }
        }
    }

    for strand in strands {
        let mut strand_seqs: Vec<(u64, Extras)> = Vec::new();
        for entries in strand.values() {
            let mut extra = Extras::default();
            let last = entries.last().unwrap();
            if last.action == Action::Spans {
                extra.bit0001 = Some(0);
            }
            if Some(entries[0].index) == first_entry_index {
                extra.bit0010 = Some(tbs_type);
            }
            if entries.len() > 1 {
                // >255 sibling entries in one record would wrap mod 256 and
                // corrupt the device position map; saturate instead.
                extra.bit0100 = Some(u8::try_from(entries.len()).unwrap_or(u8::MAX));
            }
            let parent = if entries[0].parent < 0 {
                0u32
            } else {
                entries[0].parent as u32
            };
            let mut index: i64 = entries[0].index as i64 - parent as i64;

            if !ans.is_empty() && strand_seqs.is_empty() {
                // Cross-strand: index encoded as delta from previous strand's
                // last index.
                let li = last_index.unwrap_or(0) as i64;
                index = li - entries[0].index as i64;
                if index < 0 {
                    if tbs_type == 5 {
                        index = -index;
                    } else {
                        return Err(NegativeStrandIndex);
                    }
                } else {
                    extra.bit1000 = true;
                }
            }
            last_index = Some(entries.last().unwrap().index);
            strand_seqs.push((index as u64, extra));
        }

        // Consecutive `spans` entries: keep 0b1=0 only on the last one.
        let n = strand_seqs.len();
        for i in 0..n.saturating_sub(1) {
            let cur_has = strand_seqs[i].1.bit0001.is_some();
            let next_has = strand_seqs[i + 1].1.bit0001.is_some();
            if cur_has && next_has {
                strand_seqs[i].1.bit0001 = None;
            }
        }
        ans.extend(strand_seqs);
    }
    Ok(ans)
}

fn sequences_to_bytes(seqs: &[(u64, Extras)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut flag_size: u32 = 3;
    for (val, extra) in seqs {
        out.extend(encode_tbs(*val, extra, flag_size));
        flag_size = 4;
    }
    out
}

/// Compute TBS bytes for each text record. Output is a Vec<Vec<u8>> with one
/// entry per text record (already wrapped via `encode_trailing_data` so it can
/// be appended directly to the record before the multibyte indicator byte).
pub fn build_tbs_for_records(entries: &[TbsEntry], record_lengths: &[u64]) -> Vec<Vec<u8>> {
    // Sort by start position so collect_indexing_data can early-exit.
    let mut sorted: Vec<&TbsEntry> = entries.iter().collect();
    sorted.sort_by_key(|e| e.start);

    let mut out = Vec::with_capacity(record_lengths.len());
    let mut record_start: u64 = 0;
    for &rec_length in record_lengths {
        let next_record_start = record_start + rec_length;
        let mut local: Vec<LocalEntry> = Vec::new();
        for entry in &sorted {
            if entry.start >= next_record_start {
                break;
            }
            if entry.start + entry.length <= record_start {
                continue;
            }
            local.push(fill_entry(entry, record_start, rec_length));
        }
        let strands = separate_strands(local);

        // Try tbs_type=8 first, fall back to 5 on NegativeStrandIndex.
        let seqs = match encode_strands_as_sequences(&strands, 8) {
            Ok(s) => s,
            Err(_) => encode_strands_as_sequences(&strands, 5).unwrap_or_default(),
        };
        let tbs_bytes = sequences_to_bytes(&seqs);
        out.push(encode_trailing_data(&tbs_bytes));

        record_start = next_record_start;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encint_forward_terminator_byte_high_bit() {
        // value 0 -> 0x80 (single byte, high bit set)
        assert_eq!(encint_forward(0), vec![0x80]);
        // value 1 -> 0x81
        assert_eq!(encint_forward(1), vec![0x81]);
        // value 0x80 -> 0x01 0x80
        assert_eq!(encint_forward(0x80), vec![0x01, 0x80]);
    }

    #[test]
    fn encint_backward_sets_high_bit_on_first_byte() {
        // value 1 -> 0x81 (single byte; high bit on the only byte)
        assert_eq!(encint_backward(1), vec![0x81]);
        // value 0x80 -> 0x81 0x00 (first byte has high bit + value 1, second has 0)
        assert_eq!(encint_backward(0x80), vec![0x81, 0x00]);
    }

    #[test]
    fn encode_trailing_data_self_describing_size() {
        // Empty payload: just the size byte (0x81 = 1).
        assert_eq!(encode_trailing_data(&[]), vec![0x81]);
        // 5-byte payload + 1-byte size suffix: size=6, encoded backward as 0x86.
        let raw = vec![1u8, 2, 3, 4, 5];
        let wrapped = encode_trailing_data(&raw);
        assert_eq!(wrapped, vec![1, 2, 3, 4, 5, 0x86]);
    }

    #[test]
    fn encode_fvwi_packs_value_and_flags() {
        // val=1, flags=0b10, flag_size=4 → (1<<4)|2 = 18 = 0x12 → forward vwi 0x92
        assert_eq!(encode_fvwi(1, 0b10, 4), vec![0x92]);
    }

    #[test]
    fn build_tbs_empty_entries_emits_size_byte_only() {
        let tbs = build_tbs_for_records(&[], &[4096, 4096]);
        assert_eq!(tbs.len(), 2);
        for r in &tbs {
            assert_eq!(r, &vec![0x81], "empty record should be size-1 trailer");
        }
    }

    #[test]
    fn build_tbs_single_entry_within_first_record() {
        // One top-level entry at offset 100, length 200, all inside record 0.
        let entries = vec![TbsEntry {
            index: 0,
            start: 100,
            length: 200,
            depth: 0,
            parent: -1,
        }];
        let tbs = build_tbs_for_records(&entries, &[4096, 4096]);
        assert_eq!(tbs.len(), 2);
        // Record 0 should be non-empty; record 1 should be just the size byte.
        assert!(tbs[0].len() > 1, "record 0 should have a sequence");
        assert_eq!(tbs[1], vec![0x81]);
    }
}
