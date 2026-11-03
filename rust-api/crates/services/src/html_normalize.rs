//! Shared stored-HTML normaliser (PIDASHCONV-1554).
//!
//! Django normalises stored description and comment HTML with
//! `html.tostring(html.fromstring(value), encoding="unicode")`
//! (`api/serializers/issue.py:224-229`, `:1011-1017`); any exception becomes
//! `Invalid HTML passed`. This module is the single Rust implementation,
//! used by both the intake and work-items gates. It replaces the two
//! hand-written libxml2 emulations (rule 13, PIDASHCONV-1): the tree comes
//! from an html5ever fragment parse, and only lxml's *output* conventions
//! are copied — void tags, boolean attributes, URL-attribute escaping, the
//! single-root `<div>`/`<span>` wrapping rule, and text/attribute escaping.
//!
//! Tier 1 behaviour (input real clients send — tiptap fragments, plain text,
//! single or multi-root markup) is byte-identical to lxml, pinned by the
//! corpus test (`crates/services/tests/html_normalize_corpus.rs`, outputs
//! generated from live lxml by the committed script next to it). Tier 3
//! (libxml2 repair for malformed input no client sends) intentionally
//! differs; the differences are listed in the PIDASHCONV-1554 PR body.

use html5ever::tendril::TendrilSink;
use html5ever::{local_name, ns, QualName};
use markup5ever_rcdom::{Handle, NodeData, RcDom};

/// Elements serialized without a close tag (libxml2 `htmlTagLookup` void
/// set — note `embed`, `source`, `track`, `wbr`, `keygen`, `command` and
/// `bgsound` all take close tags).
const VOID_TAGS: &[&str] = &[
    "area", "base", "basefont", "br", "col", "frame", "hr", "img", "input", "isindex", "link",
    "meta", "param",
];

/// Attributes serialized bare, value ignored, on any tag (libxml2
/// `htmlIsBooleanAttr` — note `required`, `hidden`, `async` and friends are
/// NOT in the set).
const BOOLEAN_ATTRS: &[&str] = &[
    "checked", "selected", "disabled", "readonly", "multiple", "ismap", "defer", "declare",
    "noresize", "nowrap", "noshade", "compact", "nohref",
];

/// Attributes whose values percent-encode (libxml2 `htmlAttrDumpOutput` URI
/// arm — exactly `href`/`src`/`action`, any tag, unprefixed only; `srcset`,
/// `cite`, `poster`, `data` and the rest pass through).
const URI_ATTRS: &[&str] = &["href", "src", "action"];

/// Tags whose element content serializes raw (libxml2 CDATA elements —
/// `<script>a&b</script>` round-trips byte-identical; `title`, `textarea`,
/// `iframe`, `xmp` and friends escape normally).
const RAW_TEXT_TAGS: &[&str] = &["script", "style"];

/// `defs.block_tags` (`lxml/html/defs.py:61-97`): the div-vs-span retag vote.
/// Any such tag anywhere in the fragment votes `div`.
const BLOCK_TAGS: &[&str] = &[
    "address",
    "blockquote",
    "center",
    "del",
    "div",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "ins",
    "isindex",
    "noscript",
    "p",
    "pre",
    "dir",
    "dl",
    "dt",
    "dd",
    "li",
    "menu",
    "ol",
    "ul",
    "table",
    "caption",
    "colgroup",
    "col",
    "thead",
    "tfoot",
    "tbody",
    "tr",
    "td",
    "th",
    "fieldset",
    "form",
    "legend",
    "optgroup",
    "option",
];

/// HTML-whitespace blank measure (libxml2's, not Python's): tab, LF, FF, CR,
/// space. Vertical tab and non-breaking space COUNT as content.
fn is_html_ws(ch: char) -> bool {
    matches!(ch, '\t' | '\n' | '\x0C' | '\r' | ' ')
}

fn is_html_blank(text: &str) -> bool {
    !text.chars().any(|ch| !is_html_ws(ch))
}

/// Python `str.strip` measure (what `fromstring` uses for its `.strip()`
/// checks): Unicode whitespace plus `\x1c`-`\x1f` (Rust's
/// `is_whitespace` lacks only those four).
fn is_py_strip_char(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}

fn is_py_blank(text: &str) -> bool {
    !text.chars().any(|ch| !is_py_strip_char(ch))
}

/// Parse one fragment with html5ever (scripting disabled, div context, so
/// `<noscript>` content parses as markup exactly like libxml2's script-less
/// parser). The fragment nodes hang directly under the document.
fn parse_fragment(input: &str) -> RcDom {
    let opts = html5ever::ParseOpts {
        tree_builder: html5ever::tree_builder::TreeBuilderOpts {
            scripting_enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let context = QualName::new(None, ns!(html), local_name!("div"));
    let parser = html5ever::parse_fragment(RcDom::default(), opts, context, Vec::new(), false);
    parser.one(input)
}

/// Escape text content: `&<>` only (`"`/`'` pass through raw).
fn escape_text(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
}

/// Percent-encode a URI attribute value (libxml2 `htmlAttrDumpOutput`):
/// strip leading space/tab/CR/LF (NOT vertical-tab/form-feed), then encode
/// every byte `<= 0x20`, `== 0x7F` or `>= 0x80` as UPPERCASE `%XX` (UTF-8
/// byte by byte: `é` → `%C3%A9`). Everything else — including `%`, `&`,
/// `?`, `#`, `<`, `>`, quotes — passes through for the attr escaper.
fn encode_uri(value: &str) -> String {
    let stripped = value.trim_start_matches([' ', '\t', '\r', '\n']);
    let mut out = String::with_capacity(stripped.len());
    for byte in stripped.bytes() {
        if byte <= 0x20 || byte == 0x7F || byte >= 0x80 {
            out.push_str(&format!("%{byte:02X}"));
        } else {
            out.push(byte as char);
        }
    }
    out
}

/// Serialize one attribute (libxml2 `htmlAttrDumpOutput`): booleans bare,
/// URI values encoded, quote style single iff the value holds `"` but not
/// `'`, `&<>` escaped plus `"` in double-quoted style (`'` always raw).
fn push_attr(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    if BOOLEAN_ATTRS.contains(&name) {
        return;
    }
    let encoded;
    let cooked = if URI_ATTRS.contains(&name) {
        encoded = encode_uri(value);
        encoded.as_str()
    } else {
        value
    };
    let single = cooked.contains('"') && !cooked.contains('\'');
    out.push('=');
    out.push(if single { '\'' } else { '"' });
    for ch in cooked.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if !single => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out.push(if single { '\'' } else { '"' });
}

/// The lowercased local tag name of an element handle (`None` otherwise).
fn element_name(handle: &Handle) -> Option<String> {
    match &handle.data {
        NodeData::Element { name, .. } => Some(name.local.to_string().to_ascii_lowercase()),
        _ => None,
    }
}

/// Whether any element in the subtree (inclusive) carries a block tag.
fn subtree_has_block(handle: &Handle) -> bool {
    if let Some(name) = element_name(handle) {
        if BLOCK_TAGS.contains(&name.as_str()) {
            return true;
        }
    }
    handle.children.borrow().iter().any(subtree_has_block)
}

/// Serialize a comment or PI node: `<!--data-->`; PIs (which HTML parsing
/// never produces — `<?` tokenizes as a bogus comment — but kept total)
/// as `<!--?target data?-->`.
fn push_comment(out: &mut String, handle: &Handle) {
    match &handle.data {
        NodeData::Comment { contents } => {
            out.push_str("<!--");
            out.push_str(contents.as_ref());
            out.push_str("-->");
        }
        NodeData::ProcessingInstruction { target, contents } => {
            out.push_str("<!--?");
            out.push_str(target.as_ref());
            let data = contents.to_string();
            if !data.is_empty() {
                out.push(' ');
                out.push_str(&data);
            }
            out.push_str("?-->");
        }
        _ => {}
    }
}

/// Serialize one element's attributes in document order (names
/// ASCII-lowercased — libxml2 lowercases everything, including foreign
/// `viewBox`).
fn push_attrs(out: &mut String, handle: &Handle) {
    let NodeData::Element { attrs, .. } = &handle.data else {
        return;
    };
    for attr in attrs.borrow().iter() {
        let local = attr.name.local.to_string().to_ascii_lowercase();
        let full = match &attr.name.prefix {
            Some(prefix) => format!("{}:{local}", prefix.to_string().to_ascii_lowercase()),
            None => local,
        };
        push_attr(out, &full, attr.value.as_ref());
    }
}

/// Serialize one node: elements recurse (raw for `script`/`style`), text
/// escapes, comments/PIs emit, doctypes drop.
fn push_node(out: &mut String, handle: &Handle) {
    match &handle.data {
        NodeData::Document | NodeData::Doctype { .. } => {}
        NodeData::Text { contents } => {
            escape_text(out, &contents.borrow().to_string());
        }
        NodeData::Comment { .. } | NodeData::ProcessingInstruction { .. } => {
            push_comment(out, handle);
        }
        NodeData::Element { name, .. } => {
            let tag = name.local.to_string().to_ascii_lowercase();
            push_element_named(out, handle, &tag);
        }
    }
}

/// Serialize an element under an override tag name (the div/span retag).
/// Void tags emit no close tag and no children; `script`/`style` children
/// emit raw; `<template>` fragment children serialize inline (libxml2 has
/// no template concept — content parses as normal elements).
fn push_element_named(out: &mut String, handle: &Handle, tag: &str) {
    out.push('<');
    out.push_str(tag);
    push_attrs(out, handle);
    out.push('>');
    if VOID_TAGS.contains(&tag) {
        return;
    }
    if RAW_TEXT_TAGS.contains(&tag) {
        for child in handle.children.borrow().iter() {
            if let NodeData::Text { contents } = &child.data {
                out.push_str(&contents.borrow().to_string());
            }
        }
    } else {
        for child in handle.children.borrow().iter() {
            push_node(out, child);
        }
        if tag == "template" {
            let NodeData::Element {
                template_contents, ..
            } = &handle.data
            else {
                return;
            };
            if let Some(fragment) = template_contents.borrow().as_ref() {
                for child in fragment.children.borrow().iter() {
                    push_node(out, child);
                }
            }
        }
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

/// `html.tostring(html.fromstring(value), encoding="unicode")`: `None` is
/// the `ParserError` arm (empty/whitespace-only input, lone comments —
/// comments never count as content).
///
/// Selection mirrors `lxml/html/__init__.py:839-908` for fragments: no
/// elements and HTML-blank-only text → `None`; exactly one element or
/// comment with blank surroundings → it plus its verbatim tail; text-only
/// → `<span>` with the leading HTML-whitespace run stripped; otherwise
/// `<div>` when any block tag sits in the fragment, else `<span>`.
pub fn lxml_roundtrip(value: &str) -> Option<String> {
    let dom = parse_fragment(value);
    // The fragment nodes hang under the root `<html>` element the fragment
    // algorithm appends to the document (never under the document itself).
    let root = dom
        .document
        .children
        .borrow()
        .iter()
        .find(|child| element_name(child).as_deref() == Some("html"))
        .cloned()
        .unwrap_or(dom.document.clone());
    let kept: Vec<Handle> = root
        .children
        .borrow()
        .iter()
        .filter(|child| !matches!(&child.data, NodeData::Doctype { .. }))
        .cloned()
        .collect();

    // The empty-document error: no elements, no non-blank text.
    let has_elements = kept.iter().any(|child| element_name(child).is_some());
    let has_content = kept.iter().any(|child| match &child.data {
        NodeData::Text { contents } => !is_html_blank(&contents.borrow().to_string()),
        NodeData::Element { .. } => true,
        _ => false,
    });
    if !has_elements && !has_content {
        return None;
    }

    // Single-unwrap (`:894-899`): exactly one non-text child (element or
    // comment), every direct text run Python-blank (nbsp and `\x1c` tails
    // unwrap too). The child keeps its tail — the FOLLOWING text verbatim;
    // leading runs drop with the wrapper.
    let non_text: Vec<&Handle> = kept
        .iter()
        .filter(|child| !matches!(&child.data, NodeData::Text { .. }))
        .collect();
    let texts_clean = kept.iter().all(|child| match &child.data {
        NodeData::Text { contents } => is_py_blank(&contents.borrow().to_string()),
        _ => true,
    });
    if non_text.len() == 1 && texts_clean {
        let only = non_text[0];
        let mut out = String::new();
        push_node(&mut out, only);
        let mut after = false;
        for child in &kept {
            if std::rc::Rc::ptr_eq(child, only) {
                after = true;
                continue;
            }
            if after {
                if let NodeData::Text { contents } = &child.data {
                    out.push_str(&contents.borrow().to_string());
                }
            }
        }
        return Some(out);
    }

    // Text-only: one text node becomes `<span>` with the leading
    // HTML-whitespace run stripped.
    if kept.len() == 1 {
        if let NodeData::Text { contents } = &kept[0].data {
            let text = contents.borrow().to_string();
            let stripped = text.trim_start_matches(is_html_ws);
            let mut out = String::from("<span>");
            escape_text(&mut out, stripped);
            out.push_str("</span>");
            return Some(out);
        }
    }

    // Retag: `div` when any block tag sits in the fragment, else `span`.
    // Leading HTML-whitespace strips: blank-only text nodes drop, and the
    // leading run of a first text node trims (`  a<b>x</b>` →
    // `<span>a<b>x</b></span>`).
    let mut kept = kept;
    loop {
        let drop_first = matches!(kept.first(), Some(first) if matches!(&first.data,
            NodeData::Text { contents } if is_html_blank(&contents.borrow().to_string())));
        if drop_first {
            kept.remove(0);
        } else {
            break;
        }
    }
    let mut first_text = String::new();
    let mut strip_first = false;
    if let Some(first) = kept.first() {
        if let NodeData::Text { contents } = &first.data {
            first_text = contents.borrow().to_string();
            strip_first = true;
        }
    }
    let tag = if kept.iter().any(subtree_has_block) {
        "div"
    } else {
        "span"
    };
    let mut out = String::new();
    out.push('<');
    out.push_str(tag);
    out.push('>');
    for (index, child) in kept.iter().enumerate() {
        if strip_first && index == 0 {
            escape_text(&mut out, first_text.trim_start_matches(is_html_ws));
        } else {
            push_node(&mut out, child);
        }
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_blank_inputs_are_none() {
        for input in ["", "   ", "\t\n \r", "<!--only-a-comment-->"] {
            assert_eq!(lxml_roundtrip(input), None, "input {input:?}");
        }
    }

    #[test]
    fn single_elements_pass_through() {
        for input in ["<p></p>", "<p>x</p>", "<img src=\"x\">", "<hr>"] {
            let out = lxml_roundtrip(input).expect("round-trips");
            assert_eq!(out, input, "input {input:?}");
        }
    }

    #[test]
    fn boolean_attrs_minimize_and_uris_encode() {
        assert_eq!(
            lxml_roundtrip("<input type=\"checkbox\" checked=\"checked\">").as_deref(),
            Some("<input type=\"checkbox\" checked>")
        );
        assert_eq!(
            lxml_roundtrip("<a href=\"/a b?c=d&e=f\">x</a>").as_deref(),
            Some("<a href=\"/a%20b?c=d&amp;e=f\">x</a>")
        );
    }

    #[test]
    fn multi_root_fragments_wrap() {
        assert_eq!(
            lxml_roundtrip("<p>x</p><p>y</p>").as_deref(),
            Some("<div><p>x</p><p>y</p></div>")
        );
        assert_eq!(
            lxml_roundtrip("hello").as_deref(),
            Some("<span>hello</span>")
        );
    }
}
