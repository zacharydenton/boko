//! Filepos handling for MOBI format.
//!
//! MOBI files use `filepos=NNNNN` attributes in anchor tags to reference
//! byte positions in the decompressed text stream. This module provides
//! functions matching KindleUnpack's approach:
//! 1. Collect all filepos target positions from links
//! 2. Insert `<a id="fileposNNNNN" />` anchor tags at exact byte positions
//! 3. Convert `filepos=NNNNN` to `href="#fileposNNNNN"`

use std::collections::{BTreeMap, HashSet};

/// Collect all filepos target values from `<a filepos=NNNNN>` attributes.
///
/// Returns a set of byte positions that are referenced as link targets.
/// Matches KindleUnpack's link_pattern: `<[^<>]+filepos=['"{0,1}(\d+)[^<>]*>`
pub fn collect_filepos_targets(html: &[u8]) -> HashSet<usize> {
    let mut targets = HashSet::new();
    let mut pos = 0;

    while pos < html.len() {
        // Look for filepos= pattern (may or may not have quotes)
        if pos + 8 < html.len() && html[pos..].starts_with(b"filepos=") {
            let val_start = pos + 8;
            let mut start = val_start;

            // Skip optional quote
            if start < html.len() && (html[start] == b'"' || html[start] == b'\'') {
                start += 1;
            }

            // Skip leading zeros
            while start < html.len() && html[start] == b'0' {
                start += 1;
            }

            // Parse digits
            let mut val_end = start;
            while val_end < html.len() && html[val_end].is_ascii_digit() {
                val_end += 1;
            }

            // If we only had zeros, back up to include at least one
            if val_end == start && start > val_start && html[start - 1] == b'0' {
                start -= 1;
            }

            if val_end > start {
                if let Ok(filepos) =
                    String::from_utf8_lossy(&html[start..val_end]).parse::<usize>()
                {
                    targets.insert(filepos);
                }
            } else if val_end == start {
                // Just "0" or empty after zeros
                targets.insert(0);
            }
            pos = val_end;
        } else {
            pos += 1;
        }
    }

    targets
}

/// Transform MOBI HTML matching KindleUnpack's approach:
/// 1. Insert `<a id="fileposNNNNN" />` anchor tags at exact byte positions
/// 2. Convert `filepos=NNNNN` to `href="#fileposNNNNN"`
/// 3. Convert `recindex=NNNNN` to proper image paths
///
/// This matches KindleUnpack's findAnchors() + insertHREFS() methods.
pub fn transform_mobi_html(html: &[u8], assets: &[std::path::PathBuf]) -> Vec<u8> {
    use std::collections::HashMap;

    // Step 1: Collect all filepos targets
    let targets = collect_filepos_targets(html);

    // Step 2: Build position map for anchor insertion
    // KindleUnpack inserts anchors at exact byte positions
    let mut position_map: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    for &position in &targets {
        if position > 0 && position <= html.len() {
            let anchor = format!("<a id=\"filepos{}\" />", position);
            position_map
                .entry(position)
                .or_default()
                .extend_from_slice(anchor.as_bytes());
        }
    }

    // Step 3: Build recindex -> asset path mapping
    let mut recindex_map: HashMap<String, String> = HashMap::new();
    for (i, asset) in assets.iter().enumerate() {
        let recindex = format!("{:05}", i + 1);
        recindex_map.insert(recindex, asset.to_string_lossy().to_string());
    }

    // Step 4: Insert anchors at positions (like KindleUnpack's dataList building)
    let mut with_anchors = Vec::with_capacity(html.len() + position_map.len() * 30);
    let mut last_pos = 0;

    for (&end_pos, anchor_bytes) in &position_map {
        if end_pos == 0 || end_pos > html.len() {
            continue;
        }
        with_anchors.extend_from_slice(&html[last_pos..end_pos]);
        with_anchors.extend_from_slice(anchor_bytes);
        last_pos = end_pos;
    }
    with_anchors.extend_from_slice(&html[last_pos..]);

    // Step 5: Convert filepos=NNNNN to href="#fileposNNNNN" and handle recindex
    let mut output = Vec::with_capacity(with_anchors.len());
    let mut pos = 0;

    while pos < with_anchors.len() {
        // Look for filepos= pattern
        if pos + 8 < with_anchors.len() && with_anchors[pos..].starts_with(b"filepos=") {
            let val_start = pos + 8;
            let mut start = val_start;
            let mut has_quote = false;

            // Skip optional quote
            if start < with_anchors.len()
                && (with_anchors[start] == b'"' || with_anchors[start] == b'\'')
            {
                has_quote = true;
                start += 1;
            }

            // Parse digits (including leading zeros which we strip in output)
            let digit_start = start;
            while start < with_anchors.len() && with_anchors[start].is_ascii_digit() {
                start += 1;
            }

            // Skip closing quote if present
            let mut end = start;
            if has_quote
                && end < with_anchors.len()
                && (with_anchors[end] == b'"' || with_anchors[end] == b'\'')
            {
                end += 1;
            }

            if start > digit_start {
                // Parse the number, stripping leading zeros
                let num_str = String::from_utf8_lossy(&with_anchors[digit_start..start]);
                if let Ok(filepos_num) = num_str.trim_start_matches('0').parse::<u64>() {
                    output.extend_from_slice(b"href=\"#filepos");
                    output.extend_from_slice(filepos_num.to_string().as_bytes());
                    output.push(b'"');
                    pos = end;
                    continue;
                } else if num_str.chars().all(|c| c == '0') {
                    // All zeros = position 0
                    output.extend_from_slice(b"href=\"#filepos0\"");
                    pos = end;
                    continue;
                }
            } else {
                // Empty or malformed filepos (no digits) - skip the entire attribute
                // This removes `filepos=""` or `filepos=` leaving the anchor tag
                // which will be cleaned up later or rendered as plain text
                pos = end;
                continue;
            }
        }

        // Look for recindex=" pattern
        if pos + 10 < with_anchors.len() && with_anchors[pos..].starts_with(b"recindex=\"") {
            let val_start = pos + 10;
            if let Some(val_end_rel) = with_anchors[val_start..]
                .iter()
                .position(|&b| b == b'"')
            {
                let val_end = val_start + val_end_rel;
                let recindex =
                    String::from_utf8_lossy(&with_anchors[val_start..val_end]).to_string();

                if let Some(path) = recindex_map.get(&recindex) {
                    output.extend_from_slice(b"src=\"");
                    output.extend_from_slice(path.as_bytes());
                    output.push(b'"');
                    pos = val_end + 1;
                    continue;
                }
            }
        }

        // Copy byte as-is
        output.push(with_anchors[pos]);
        pos += 1;
    }

    // Step 6: Remove empty anchors (like KindleUnpack does)
    remove_empty_anchors(&mut output);

    output
}

/// Remove empty anchor tags: `<a />` and `<a></a>`
fn remove_empty_anchors(html: &mut Vec<u8>) {
    // This is a simple implementation - could be optimized
    let html_str = String::from_utf8_lossy(html);

    // Remove <a /> and <a  /> patterns
    let cleaned = html_str
        .replace("<a />", "")
        .replace("<a  />", "")
        .replace("<a></a>", "")
        .replace("<a ></a>", "");

    html.clear();
    html.extend_from_slice(cleaned.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_collect_filepos_targets() {
        let html = b"<a filepos=1234>Link1</a> text <a filepos=5678>Link2</a>";
        let targets = collect_filepos_targets(html);

        assert!(targets.contains(&1234));
        assert!(targets.contains(&5678));
        assert_eq!(targets.len(), 2);
    }

    #[test]
    fn test_collect_filepos_with_quotes() {
        let html = b"<a filepos=\"0001234\">Link</a>";
        let targets = collect_filepos_targets(html);

        assert!(targets.contains(&1234));
    }

    #[test]
    fn test_transform_inserts_anchor_at_position() {
        // Position 50 should have an anchor inserted
        let mut html = vec![b' '; 100];
        html[0..6].copy_from_slice(b"<html>");
        html[50..60].copy_from_slice(b"<p>Hello</");
        // Add a link pointing to position 50
        let link = b"<a filepos=50>Link</a>";
        html.extend_from_slice(link);

        let result = transform_mobi_html(&html, &[]);
        let result_str = String::from_utf8_lossy(&result);

        // Should have anchor at position 50
        assert!(
            result_str.contains("<a id=\"filepos50\" />"),
            "Should insert anchor: {}",
            result_str
        );
        // Should convert filepos to href
        assert!(
            result_str.contains("href=\"#filepos50\""),
            "Should convert href: {}",
            result_str
        );
    }

    #[test]
    fn test_transform_filepos_to_href() {
        let html = b"<a filepos=1234>Link</a>";
        let result = transform_mobi_html(html, &[]);
        let result_str = String::from_utf8_lossy(&result);

        assert!(result_str.contains("href=\"#filepos1234\""));
        assert!(!result_str.contains("filepos="));
    }

    #[test]
    fn test_transform_recindex() {
        let assets = vec![PathBuf::from("images/image_0000.jpg")];
        let html = b"<img recindex=\"00001\">";
        let result = transform_mobi_html(html, &assets);
        let result_str = String::from_utf8_lossy(&result);

        assert!(result_str.contains("src=\"images/image_0000.jpg\""));
        assert!(!result_str.contains("recindex"));
    }

    #[test]
    fn test_transform_with_leading_zeros() {
        let html = b"<a filepos=0000100>Link</a>";
        let result = transform_mobi_html(html, &[]);
        let result_str = String::from_utf8_lossy(&result);

        // Should strip leading zeros in href
        assert!(result_str.contains("href=\"#filepos100\""));
    }

    #[test]
    fn test_transform_empty_filepos_quoted() {
        // Empty filepos with quotes should be removed, leaving plain anchor
        let html = b"<a filepos=\"\">Link text</a>";
        let result = transform_mobi_html(html, &[]);
        let result_str = String::from_utf8_lossy(&result);

        // The empty filepos="" attribute should be stripped
        assert!(
            !result_str.contains("filepos"),
            "Empty filepos should be removed: {}",
            result_str
        );
        // The link text should remain
        assert!(
            result_str.contains("Link text"),
            "Link text should remain: {}",
            result_str
        );
    }

    #[test]
    fn test_transform_empty_filepos_unquoted() {
        // Empty filepos without quotes (malformed) should be handled
        let html = b"<a filepos=>Link text</a>";
        let result = transform_mobi_html(html, &[]);
        let result_str = String::from_utf8_lossy(&result);

        // The empty filepos= attribute should be stripped
        assert!(
            !result_str.contains("filepos"),
            "Empty filepos should be removed: {}",
            result_str
        );
        // The link text should remain
        assert!(
            result_str.contains("Link text"),
            "Link text should remain: {}",
            result_str
        );
    }

    #[test]
    fn test_transform_whitespace_only_filepos() {
        // filepos with only whitespace should be handled
        let html = b"<a filepos=\"  \">Link text</a>";
        let result = transform_mobi_html(html, &[]);
        let result_str = String::from_utf8_lossy(&result);

        // The whitespace-only filepos should be stripped
        assert!(
            !result_str.contains("filepos"),
            "Whitespace-only filepos should be removed: {}",
            result_str
        );
    }
}
