//! Split synthesized XHTML flows at element boundaries and relocate links.

use std::collections::HashMap;
use std::io;

use quick_xml::Reader;
use quick_xml::events::Event;

use super::html_synth::escape_xml;
use super::normalize::{ChapterContent, xml_unescape};
use crate::model::TocEntry;

#[derive(Clone)]
struct OpenElement {
    name: String,
    start_tag: String,
}

pub(super) struct SplitFlows {
    pub(super) chapters: Vec<ChapterContent>,
    first_file: HashMap<String, String>,
    anchors: HashMap<(String, String), String>,
}

impl SplitFlows {
    pub(super) fn new(chapters: &[ChapterContent], max_bytes: usize) -> io::Result<Self> {
        let mut result = Self {
            chapters: Vec::new(),
            first_file: HashMap::new(),
            anchors: HashMap::new(),
        };
        let mut source_files = Vec::new();

        for (chapter_index, chapter) in chapters.iter().enumerate() {
            let old_file = format!("chapter_{chapter_index}.xhtml");
            let parts = split_xhtml(&chapter.document, max_bytes)?;
            for (part_index, document) in parts.into_iter().enumerate() {
                let new_file = format!("chapter_{}.xhtml", result.chapters.len());
                if part_index == 0 {
                    result.first_file.insert(old_file.clone(), new_file.clone());
                }
                for id in collect_ids(&document)? {
                    result
                        .anchors
                        .insert((old_file.clone(), id), new_file.clone());
                }
                result.chapters.push(ChapterContent {
                    id: chapter.id,
                    source_path: chapter.source_path.clone(),
                    document,
                });
                source_files.push(old_file.clone());
            }
        }

        for (chapter, source_file) in result.chapters.iter_mut().zip(&source_files) {
            chapter.document = rewrite_hrefs(
                &chapter.document,
                source_file,
                &result.first_file,
                &result.anchors,
            );
        }
        Ok(result)
    }

    pub(super) fn rewrite_toc(&self, entries: &[TocEntry]) -> Vec<TocEntry> {
        entries
            .iter()
            .map(|entry| {
                let mut entry = entry.clone();
                entry.href = self.relocate(&entry.href);
                entry.children = self.rewrite_toc(&entry.children);
                entry
            })
            .collect()
    }

    fn relocate(&self, href: &str) -> String {
        relocate(href, None, &self.first_file, &self.anchors)
    }
}

fn relocate(
    href: &str,
    source_file: Option<&str>,
    first_file: &HashMap<String, String>,
    anchors: &HashMap<(String, String), String>,
) -> String {
    if href.is_empty() || href.contains("://") || href.starts_with("mailto:") {
        return href.to_string();
    }
    let (file, fragment) = match href.split_once('#') {
        Some(pair) => pair,
        None => (href, ""),
    };
    let file = if file.is_empty() {
        match source_file {
            Some(source_file) if !fragment.is_empty() => source_file,
            _ => return href.to_string(),
        }
    } else {
        file
    };
    let target = if fragment.is_empty() {
        first_file.get(file)
    } else {
        let decoded = percent_encoding::percent_decode_str(fragment).decode_utf8_lossy();
        anchors
            .get(&(file.to_string(), decoded.into_owned()))
            .or_else(|| first_file.get(file))
    };
    match target {
        Some(target) if fragment.is_empty() => target.clone(),
        Some(target) => format!("{target}#{fragment}"),
        None => href.to_string(),
    }
}

fn split_xhtml(document: &str, max_bytes: usize) -> io::Result<Vec<String>> {
    if document.len() <= max_bytes {
        return Ok(vec![document.to_string()]);
    }
    let body_start = document
        .find("<body>")
        .map(|index| index + "<body>".len())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "XHTML body start is missing"))?;
    let body_end = document
        .rfind("</body>")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "XHTML body end is missing"))?;
    let prefix = &document[..body_start];
    let body = &document[body_start..body_end];
    let suffix = &document[body_end..];
    let mut reader = Reader::from_str(body);
    let mut stack: Vec<OpenElement> = Vec::new();
    let mut parts = Vec::new();
    let mut chunk_start = 0;
    let mut reopened = String::new();
    let mut last_boundary: Option<(usize, Vec<OpenElement>)> = None;

    loop {
        let event_start = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(io::Error::other)?;
        let event_end = reader.buffer_position() as usize;
        let boundary = match &event {
            Event::Start(start) => {
                stack.push(OpenElement {
                    name: String::from_utf8_lossy(start.name().as_ref()).into_owned(),
                    start_tag: body[event_start..event_end].to_string(),
                });
                false
            }
            Event::End(_) => {
                stack.pop();
                true
            }
            Event::Empty(_) => true,
            Event::Eof => break,
            _ => false,
        };
        let inside_paragraph_break = matches!(
            &event,
            Event::Empty(element) if element.name().as_ref() == b"br"
                && stack.last().is_some_and(|element| element.name == "p")
        );
        if !boundary
            || stack.iter().any(|element| {
                !matches!(element.name.as_str(), "div" | "section" | "article")
                    && !(inside_paragraph_break && element.name == "p")
            })
        {
            continue;
        }
        let closing: String = stack
            .iter()
            .rev()
            .map(|element| format!("</{}>", element.name))
            .collect();
        let estimated = prefix.len()
            + reopened.len()
            + event_end.saturating_sub(chunk_start)
            + closing.len()
            + suffix.len();
        if estimated < max_bytes {
            last_boundary = Some((event_end, stack.clone()));
            continue;
        }
        if body[chunk_start..event_end].trim().is_empty() {
            continue;
        }
        let (split_at, split_stack) = last_boundary
            .take()
            .filter(|(at, _)| *at > chunk_start)
            .unwrap_or((event_end, stack.clone()));
        let closing: String = split_stack
            .iter()
            .rev()
            .map(|element| format!("</{}>", element.name))
            .collect();
        let mut part = String::with_capacity(estimated);
        part.push_str(prefix);
        part.push_str(&reopened);
        part.push_str(&body[chunk_start..split_at]);
        part.push_str(&closing);
        part.push_str(suffix);
        parts.push(part);
        chunk_start = split_at;
        reopened.clear();
        for element in &split_stack {
            reopened.push_str(&without_id(&element.start_tag));
        }
        if split_at < event_end {
            last_boundary = Some((event_end, stack.clone()));
        }
    }

    if has_remaining_content(&body[chunk_start..]) || parts.is_empty() {
        let mut part = String::new();
        part.push_str(prefix);
        part.push_str(&reopened);
        part.push_str(&body[chunk_start..]);
        part.push_str(suffix);
        parts.push(part);
    }
    Ok(parts)
}

fn without_id(tag: &str) -> String {
    let mut output = tag.to_string();
    for name in [" id=\"", " xml:id=\""] {
        if let Some(start) = output.find(name)
            && let Some(end) = output[start + name.len()..].find('"')
        {
            output.replace_range(start..start + name.len() + end + 1, "");
        }
    }
    output
}

fn has_remaining_content(rest: &str) -> bool {
    let mut reader = Reader::from_str(rest);
    loop {
        match reader.read_event() {
            Ok(Event::Start(_)) => return true,
            Ok(Event::Empty(_)) => return true,
            Ok(Event::Text(text)) => {
                if !text.into_inner().iter().all(u8::is_ascii_whitespace) {
                    return true;
                }
            }
            Ok(Event::Eof) | Err(_) => return false,
            _ => {}
        }
    }
}

fn collect_ids(document: &str) -> io::Result<Vec<String>> {
    let mut reader = Reader::from_str(document);
    let mut ids = Vec::new();
    loop {
        let event = reader.read_event().map_err(io::Error::other)?;
        match event {
            Event::Start(start) | Event::Empty(start) => {
                for attribute in start.attributes().with_checks(false) {
                    let attribute = attribute.map_err(io::Error::other)?;
                    if attribute.key.as_ref() == b"id" {
                        ids.push(
                            attribute
                                .unescape_value()
                                .map_err(io::Error::other)?
                                .into_owned(),
                        );
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(ids)
}

fn rewrite_hrefs(
    document: &str,
    source_file: &str,
    first_file: &HashMap<String, String>,
    anchors: &HashMap<(String, String), String>,
) -> String {
    const NEEDLE: &str = " href=\"";
    let mut output = String::with_capacity(document.len());
    let mut rest = document;
    while let Some(index) = rest.find(NEEDLE) {
        let (before, after) = rest.split_at(index + NEEDLE.len());
        output.push_str(before);
        if let Some(end) = after.find('"') {
            let old = xml_unescape(&after[..end]);
            let new = relocate(&old, Some(source_file), first_file, anchors);
            if old == new {
                output.push_str(&after[..end]);
            } else {
                output.push_str(&escape_xml(&new));
            }
            rest = &after[end..];
        } else {
            rest = after;
        }
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::ChapterId;

    #[test]
    fn splits_large_nested_flow_and_relocates_links() {
        let body = (0..12)
            .map(|index| {
                format!(
                    "<p id=\"part{index}\"><a href=\"chapter_0.xhtml#part11\">next</a>{}</p>",
                    "word ".repeat(40)
                )
            })
            .collect::<String>();
        let document = format!(
            "<?xml version=\"1.0\"?><html><head><title>Test</title></head><body><div>{body}</div></body></html>"
        );
        let original = ChapterContent {
            id: ChapterId(0),
            source_path: "one.xhtml".into(),
            document,
        };

        let split = SplitFlows::new(&[original], 1024).unwrap();
        assert!(split.chapters.len() > 1);
        let target = split.relocate("chapter_0.xhtml#part11");
        assert_ne!(target, "chapter_0.xhtml#part11");
        for chapter in &split.chapters {
            assert!(chapter.document.contains(&format!("href=\"{target}\"")));
            let mut reader = Reader::from_str(&chapter.document);
            loop {
                if matches!(reader.read_event().unwrap(), Event::Eof) {
                    break;
                }
            }
        }
        assert_eq!(
            split.rewrite_toc(&[TocEntry::new("last", "chapter_0.xhtml#part11")])[0].href,
            target
        );
    }

    #[test]
    fn keeps_short_flow_unchanged() {
        let document = "<html><body><p>short</p></body></html>";
        assert_eq!(split_xhtml(document, 1024).unwrap(), vec![document]);
    }

    #[test]
    fn splits_large_paragraph_at_breaks_without_repeating_id() {
        let body = format!(
            "<p id=\"long\"><a href=\"#end\">start</a>{}<span id=\"end\">end</span></p>",
            "word word word<br/>".repeat(100)
        );
        let original = ChapterContent {
            id: ChapterId(0),
            source_path: "one.xhtml".into(),
            document: format!("<html><body>{body}</body></html>"),
        };
        for part in split_xhtml(&original.document, 512).unwrap() {
            assert!(part.len() <= 512, "{}", part.len());
        }
        let split = SplitFlows::new(&[original], 512).unwrap();
        assert!(split.chapters.len() > 1);
        let target = split.relocate("chapter_0.xhtml#end");
        assert_ne!(target, "chapter_0.xhtml#end");
        let mut all = String::new();
        for chapter in &split.chapters {
            assert!(chapter.document.len() < 600);
            assert!(
                chapter.document.contains(&format!("href=\"{target}\""))
                    || !chapter.document.contains("start")
            );
            let mut reader = Reader::from_str(&chapter.document);
            loop {
                if matches!(reader.read_event().unwrap(), Event::Eof) {
                    break;
                }
            }
            all.push_str(&chapter.document);
        }
        assert_eq!(all.matches("id=\"long\"").count(), 1);
        assert_eq!(all.matches("<br/>").count(), 100);
        assert_eq!(all.matches("word word word").count(), 100);
    }

    #[test]
    fn relocates_escaped_and_percent_encoded_fragments() {
        let body = format!(
            "<p><a href=\"#a%20b\">space</a><a href=\"#a&amp;b\">amp</a>{}</p><p id=\"a b\">target</p><p id=\"a&amp;b\">target</p>",
            "word ".repeat(250)
        );
        let original = ChapterContent {
            id: ChapterId(0),
            source_path: "one.xhtml".into(),
            document: format!("<html><body>{body}</body></html>"),
        };
        let split = SplitFlows::new(&[original], 1024).unwrap();
        assert!(split.chapters.len() > 1);
        let first = &split.chapters[0].document;
        assert!(first.contains("href=\"chapter_1.xhtml#a%20b\""));
        assert!(first.contains("href=\"chapter_1.xhtml#a&amp;b\""));
    }
}
