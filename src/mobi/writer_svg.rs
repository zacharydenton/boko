//! Preserve SVG geometry and styling; only repair archive image references.
//!
//! Replacing an SVG with an img changes coordinate systems, clipping, CSS
//! selectors and anchor targets even when it contains only one image. #34
//! demonstrates broken references and namespaces, not equivalence to an img.

use quick_xml::{NsReader, events::Event, name::ResolveResult};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

const SVG: &str = "http://www.w3.org/2000/svg";
const XLINK: &str = "http://www.w3.org/1999/xlink";

/// Resolve a local archive URL against a document URL (including xml:base).
/// Remote URLs, fragment references and queries aren't archive resource keys.
pub(super) fn local_reference(base: &str, url: &str) -> Option<String> {
    if [base, url]
        .iter()
        .any(|s| s.contains(':') || s.contains('?') || s.contains('#') || s.starts_with("//"))
    {
        return None;
    }
    let url = percent_encoding::percent_decode_str(url)
        .decode_utf8()
        .ok()?;
    let dir = base.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut path = super::resolve_href(dir, &url);
    if url.ends_with('/') {
        path.push('/');
    }
    Some(path)
}

struct Scan {
    references: Vec<(Range<usize>, String)>,
    svg: Vec<Range<usize>>,
    prefixes: HashSet<String>,
}

fn scan(
    html: &[u8],
    href: &str,
    resources: &HashMap<String, usize>,
    book_resources: &HashMap<String, crate::model::Resource>,
) -> Option<Scan> {
    let mut reader = NsReader::from_str(std::str::from_utf8(html).ok()?);
    let mut result = Scan {
        references: Vec::new(),
        svg: Vec::new(),
        prefixes: HashSet::new(),
    };
    let mut bases = vec![Some(href.to_string())];
    let mut depth: usize = 0;
    let mut svg_root = None;
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().ok()?;
        let end = reader.buffer_position() as usize;
        match event {
            Event::Start(ref tag) | Event::Empty(ref tag) => {
                let empty = matches!(event, Event::Empty(_));
                let (namespace, local) = reader.resolver().resolve_element(tag.name());
                let is_svg = matches!(namespace, ResolveResult::Bound(ns) if ns.as_ref() == SVG);
                let bare_svg =
                    matches!(namespace, ResolveResult::Unbound) && local.as_ref() == "svg";
                if svg_root.is_none() && local.as_ref() == "svg" && (is_svg || bare_svg) {
                    svg_root = Some((start, depth));
                }
                let attributes = tag.attributes().collect::<Result<Vec<_>, _>>().ok()?;
                {
                    // A prefixed element retained outside SVG (e.g. EPUB
                    // ops:switch) also needs its inherited binding.
                    let svg_attrs = attributes
                        .iter()
                        .filter(|_| svg_root.is_some())
                        .map(|a| a.key);
                    for name in std::iter::once(tag.name()).chain(svg_attrs) {
                        if let Some((prefix, _)) = name.as_ref().split_once(':')
                            && prefix != "xmlns"
                            && prefix != "xml"
                        {
                            result.prefixes.insert(prefix.to_string());
                        }
                    }
                }
                let mut base = bases.last()?.clone();
                if let Some(attr) = attributes.iter().find(|a| a.key.as_ref() == "xml:base") {
                    base = base.and_then(|base| {
                        let value = attr
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .ok()?;
                        local_reference(&base, &value)
                    });
                }
                if local.as_ref() == "image"
                    && (is_svg
                        || (svg_root.is_some() && matches!(namespace, ResolveResult::Unbound)))
                {
                    for attr in &attributes {
                        let (ns, name) = reader.resolver().resolve_attribute(attr.key);
                        if name.as_ref() != "href"
                            || !matches!(
                                ns,
                                ResolveResult::Unbound
                                    | ResolveResult::Bound(quick_xml::name::Namespace(XLINK))
                            )
                        {
                            continue;
                        }
                        let Some(base) = &base else { continue };
                        let Ok(value) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        else {
                            continue;
                        };
                        let Some(path) = local_reference(base, &value) else {
                            continue;
                        };
                        let Some(index) = resources.get(&path) else {
                            continue;
                        };
                        // The slice reader borrows attribute values from html.
                        // Edit just the value, preserving quotes and all other
                        // markup byte-for-byte. Check bounds before using offsets.
                        let a =
                            (attr.value.as_ptr() as usize).checked_sub(html.as_ptr() as usize)?;
                        let b = a.checked_add(attr.value.len())?;
                        if a < start || b > end {
                            return None;
                        }
                        let mut buf = [0; 4];
                        super::write_base32_4(*index, &mut buf);
                        let mime = book_resources
                            .get(&path)
                            .map_or_else(|| crate::util::guess_media_type(&path), |r| r.media_type);
                        result.references.push((
                            a..b,
                            format!(
                                "kindle:embed:{}?mime={mime}",
                                std::str::from_utf8(&buf).unwrap()
                            ),
                        ));
                    }
                }
                if empty {
                    if let Some((a, d)) = svg_root
                        && d == depth
                    {
                        result.svg.push(a..end);
                        svg_root = None;
                    }
                } else {
                    bases.push(base);
                    depth += 1;
                }
            }
            Event::End(_) => {
                depth = depth.checked_sub(1)?;
                bases.pop();
                if let Some((a, d)) = svg_root
                    && d == depth
                {
                    result.svg.push(a..end);
                    svg_root = None;
                }
            }
            Event::Eof => return (depth == 0).then_some(result),
            _ => {}
        }
    }
}

pub(super) fn rewrite_image_references(
    html: &[u8],
    href: &str,
    resources: &HashMap<String, usize>,
    book_resources: &HashMap<String, crate::model::Resource>,
) -> Vec<u8> {
    let Some(scan) = scan(html, href, resources, book_resources) else {
        return html.to_vec();
    };
    let mut out = Vec::with_capacity(html.len());
    let mut pos = 0;
    for (range, replacement) in scan.references {
        out.extend_from_slice(&html[pos..range.start]);
        out.extend_from_slice(replacement.as_bytes());
        pos = range.end;
    }
    out.extend_from_slice(&html[pos..]);
    out
}

pub(super) fn context(html: &[u8]) -> Option<(Vec<Range<usize>>, HashSet<String>)> {
    scan(html, "", &HashMap::new(), &HashMap::new()).map(|s| (s.svg, s.prefixes))
}
