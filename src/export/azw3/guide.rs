use super::*;

/// Flatten hierarchical TOC into linear list.
pub(super) fn flatten_toc(
    entries: &[TocEntry],
    text_length: u32,
    id_map: &HashMap<(String, String), String>,
    aid_offset_map: &HashMap<String, (usize, usize, usize)>,
    filepos_map: &HashMap<String, Vec<(usize, String)>>,
) -> Vec<NcxBuildEntry> {
    struct TempEntry {
        pos: u32,
        length: u32,
        label: String,
        depth: u32,
        parent: i32,
        children: Vec<usize>,
        pos_fid: (u32, u32),
    }

    let mut result: Vec<TempEntry> = Vec::new();

    #[allow(clippy::too_many_arguments)]
    fn flatten_recursive(
        entries: &[TocEntry],
        depth: u32,
        parent_idx: i32,
        text_length: u32,
        id_map: &HashMap<(String, String), String>,
        aid_offset_map: &HashMap<String, (usize, usize, usize)>,
        filepos_map: &HashMap<String, Vec<(usize, String)>>,
        result: &mut Vec<TempEntry>,
    ) {
        for entry in entries {
            let current_idx = result.len();

            let (file, fragment) = if let Some(hash_pos) = entry.href.find('#') {
                (
                    entry.href[..hash_pos].to_string(),
                    entry.href[hash_pos + 1..].to_string(),
                )
            } else {
                (entry.href.clone(), String::new())
            };

            // Look up the aid's (chunk_seq, offset_in_chunk, offset_in_text) for this TOC entry
            let aid_entry = if fragment.starts_with("filepos") {
                resolve_filepos_entry(&file, &fragment, filepos_map, aid_offset_map)
            } else {
                id_map
                    .get(&(file.clone(), fragment.clone()))
                    .or_else(|| id_map.get(&(file.clone(), String::new())))
                    .and_then(|aid| aid_offset_map.get(aid))
                    .copied()
            };

            let (fid, off_in_chunk, pos) = aid_entry
                .map(|(seq, off_in_chunk, off_text)| {
                    (seq as u32, off_in_chunk as u32, off_text as u32)
                })
                .unwrap_or((0, 0, 0));

            result.push(TempEntry {
                pos,
                length: text_length.saturating_sub(pos),
                label: entry.title.clone(),
                depth,
                parent: parent_idx,
                children: Vec::new(),
                pos_fid: (fid, off_in_chunk),
            });

            if parent_idx >= 0 {
                result[parent_idx as usize].children.push(current_idx);
            }

            flatten_recursive(
                &entry.children,
                depth + 1,
                current_idx as i32,
                text_length,
                id_map,
                aid_offset_map,
                filepos_map,
                result,
            );
        }
    }

    flatten_recursive(
        entries,
        0,
        -1,
        text_length,
        id_map,
        aid_offset_map,
        filepos_map,
        &mut result,
    );

    // Recompute lengths from the hierarchy: each entry covers up to the next
    // entry at the same or shallower depth (matches calibre's writer8/main.py).
    // The old default of `text_length - pos` made every entry span the whole
    // book, which breaks TBS strand classification and Kindle navigation.
    let n = result.len();
    let mut new_lengths = vec![0u32; n];
    for i in 0..n {
        let pos_i = result[i].pos;
        let depth_i = result[i].depth;
        let next_start = result
            .iter()
            .filter(|e| e.depth <= depth_i && e.pos > pos_i)
            .map(|e| e.pos)
            .min()
            .unwrap_or(text_length);
        new_lengths[i] = next_start.saturating_sub(pos_i);
    }
    for (i, len) in new_lengths.into_iter().enumerate() {
        result[i].length = len;
    }

    result
        .into_iter()
        .map(|e| NcxBuildEntry {
            pos: e.pos,
            length: e.length,
            label: e.label,
            depth: e.depth,
            parent: e.parent,
            first_child: e.children.first().map(|&i| i as i32).unwrap_or(-1),
            last_child: e.children.last().map(|&i| i as i32).unwrap_or(-1),
            pos_fid: Some(e.pos_fid),
        })
        .collect()
}

/// Map a boko `LandmarkType` to the KF8 guide reference type string Kindle
/// expects ("cover", "start", "toc", "notes", etc.). Returning `None` means
/// the landmark won't be emitted as a guide entry.
pub(super) fn landmark_to_guide_type(lt: crate::model::LandmarkType) -> Option<&'static str> {
    use crate::model::LandmarkType::*;
    Some(match lt {
        Cover => "cover",
        TitlePage => "title-page",
        Toc => "toc",
        StartReading => "start",
        BodyMatter => "text",
        FrontMatter => "preface",
        BackMatter => "backmatter",
        Acknowledgements => "acknowledgements",
        Bibliography => "bibliography",
        Glossary => "glossary",
        Index => "index",
        Preface => "preface",
        Endnotes => "notes",
        Loi => "loi",
        Lot => "lot",
    })
}

/// Build K8 guide entries from book landmarks. Each landmark resolves to a
/// `(fid, offset)` pair via the chunker's `id_map`/`aid_offset_map`.
pub(super) fn collect_guide_entries(
    landmarks: &[crate::model::Landmark],
    cover_image: Option<&str>,
    id_map: &HashMap<(String, String), String>,
    aid_offset_map: &HashMap<String, (usize, usize, usize)>,
) -> Vec<GuideBuildEntry> {
    let mut entries: Vec<GuideBuildEntry> = Vec::new();
    let mut seen_types: HashSet<String> = HashSet::new();

    for landmark in landmarks {
        let Some(guide_type) = landmark_to_guide_type(landmark.landmark_type) else {
            continue;
        };
        if !seen_types.insert(guide_type.to_string()) {
            continue;
        }

        let (file, fragment) = match landmark.href.find('#') {
            Some(i) => (
                landmark.href[..i].to_string(),
                landmark.href[i + 1..].to_string(),
            ),
            None => (landmark.href.clone(), String::new()),
        };

        let pos_fid = id_map
            .get(&(file.clone(), fragment.clone()))
            .or_else(|| id_map.get(&(file.clone(), String::new())))
            .and_then(|aid| aid_offset_map.get(aid))
            .map(|&(seq, off_in_chunk, _)| (seq as u32, off_in_chunk as u32));

        if let Some(pf) = pos_fid {
            entries.push(GuideBuildEntry {
                guide_type: guide_type.to_string(),
                title: if landmark.label.is_empty() {
                    guide_type.to_string()
                } else {
                    landmark.label.clone()
                },
                pos_fid: pf,
            });
        }
    }

    // Synthesize a "start" entry pointing to the first spine file if none was
    // declared — Kindle uses this to decide where to open the book.
    if !seen_types.contains("start")
        && let Some((_, (seq, off, _))) = aid_offset_map.iter().min_by_key(|(_, (_, _, abs))| *abs)
    {
        entries.push(GuideBuildEntry {
            guide_type: "start".to_string(),
            title: "Beginning".to_string(),
            pos_fid: (*seq as u32, *off as u32),
        });
    }

    // Guide entries should be sorted by type — Kindle's binary search of the
    // index depends on it (calibre comments: "Needed by the Kindle").
    entries.sort_by(|a, b| a.guide_type.cmp(&b.guide_type));
    let _ = cover_image; // currently unused; reserved for future cover-page synthesis
    entries
}

/// Resolve MOBI filepos anchor to the full (seq_num, offset_in_chunk, offset_in_text)
/// entry from the aid_offset_map.
///
/// MOBI files use `#fileposNNN` anchors where NNN is a byte position in the
/// original HTML content. We use the filepos_map to find the aid that was
/// closest to that position, then return its full entry.
pub(super) fn resolve_filepos_entry(
    file: &str,
    fragment: &str,
    filepos_map: &HashMap<String, Vec<(usize, String)>>,
    aid_offset_map: &HashMap<String, (usize, usize, usize)>,
) -> Option<(usize, usize, usize)> {
    let filepos_str = fragment.strip_prefix("filepos")?;
    let target_pos: usize = filepos_str.parse().ok()?;

    let positions = filepos_map.get(file)?;
    if positions.is_empty() {
        return None;
    }

    let idx = match positions.binary_search_by_key(&target_pos, |(pos, _)| *pos) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    };

    let (_, aid) = &positions[idx];
    aid_offset_map.get(aid).copied()
}

/// Resolve MOBI filepos anchor to (fid, offset) for link resolution.
///
/// Same lookup as [`resolve_filepos_entry`], dropping the third tuple element
/// (only seq_num and offset_in_chunk are needed for the
/// kindle:pos:fid:XXXX:off:YYYYYY link format).
pub(super) fn resolve_filepos_to_offset(
    file: &str,
    fragment: &str,
    filepos_map: &HashMap<String, Vec<(usize, String)>>,
    aid_offset_map: &HashMap<String, (usize, usize, usize)>,
) -> Option<(usize, usize)> {
    resolve_filepos_entry(file, fragment, filepos_map, aid_offset_map)
        .map(|(seq_num, offset_in_chunk, _)| (seq_num, offset_in_chunk))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_filepos_entry_exact_match() {
        let mut filepos_map = HashMap::new();
        filepos_map.insert(
            "content.html".to_string(),
            vec![
                (100, "0001".to_string()),
                (200, "0002".to_string()),
                (300, "0003".to_string()),
            ],
        );

        let mut aid_offset_map = HashMap::new();
        aid_offset_map.insert("0001".to_string(), (0, 50, 100));
        aid_offset_map.insert("0002".to_string(), (0, 150, 200));
        aid_offset_map.insert("0003".to_string(), (0, 250, 300));

        let result =
            resolve_filepos_entry("content.html", "filepos200", &filepos_map, &aid_offset_map);
        assert_eq!(result, Some((0, 150, 200)));
    }

    #[test]
    fn test_resolve_filepos_entry_nearest_before() {
        let mut filepos_map = HashMap::new();
        filepos_map.insert(
            "content.html".to_string(),
            vec![(100, "0001".to_string()), (200, "0002".to_string())],
        );

        let mut aid_offset_map = HashMap::new();
        aid_offset_map.insert("0001".to_string(), (0, 50, 100));
        aid_offset_map.insert("0002".to_string(), (1, 25, 200));

        let result =
            resolve_filepos_entry("content.html", "filepos250", &filepos_map, &aid_offset_map);
        assert_eq!(result, Some((1, 25, 200)));
    }

    #[test]
    fn test_resolve_filepos_entry_invalid_fragment() {
        let filepos_map = HashMap::new();
        let aid_offset_map = HashMap::new();

        assert_eq!(
            resolve_filepos_entry("content.html", "anchor123", &filepos_map, &aid_offset_map),
            None
        );
        assert_eq!(
            resolve_filepos_entry("content.html", "fileposXYZ", &filepos_map, &aid_offset_map),
            None
        );
    }

    #[test]
    fn test_resolve_filepos_to_offset() {
        let mut filepos_map = HashMap::new();
        filepos_map.insert(
            "content.html".to_string(),
            vec![(100, "0001".to_string()), (500, "0002".to_string())],
        );

        let mut aid_offset_map = HashMap::new();
        aid_offset_map.insert("0001".to_string(), (0, 50, 100));
        aid_offset_map.insert("0002".to_string(), (1, 25, 500));

        // Position 450 should resolve to aid at 100 (nearest before)
        let result =
            resolve_filepos_to_offset("content.html", "filepos450", &filepos_map, &aid_offset_map);
        assert_eq!(result, Some((0, 50))); // seq_num=0, offset_in_chunk=50

        // Exact match at 500
        let result =
            resolve_filepos_to_offset("content.html", "filepos500", &filepos_map, &aid_offset_map);
        assert_eq!(result, Some((1, 25))); // seq_num=1, offset_in_chunk=25
    }
}
