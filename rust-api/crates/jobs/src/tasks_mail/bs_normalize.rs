//! `str(BeautifulSoup(html, "html.parser"))`, ported from CPython 3.12.10 /
//! bs4 4.12.3 (`email_notification_task.py:130-140` runs every mention
//! content string through it, even with no `mention-component` in it).
//!
//! 3.12.10 carries the classic `html.parser` (`commentclose` is
//! `--\s*>`, `CDATA_CONTENT_ELEMENTS` is script/style only, incomplete
//! markup at EOF becomes text). Later 3.12.x and current distro builds
//! carry the HTML5 rewrite, which parses differently (PIDASHCONV-1693).
//! The golden fixture records the recording interpreter in `_python` /
//! `_bs4`, checked by the golden test below.
//!
//! [`normalize_bs`] parses `html` exactly like that parser (greedy
//! no-backtrack entity refs in text, `html.unescape` in attributes, void
//! elements, script/style raw text, bogus comments, the EOF
//! to-next-`>`/`<`/one-char data rule, whitespace-only collapsing
//! outside `<pre>`/`<textarea>`) and serializes exactly like bs4
//! (entities decoded then minimally re-escaped, void elements as `<x/>`,
//! attributes lowercased, sorted, double-quoted). Every rule is
//! probe-verified; the full probe corpus plus a whole-table sweep pins
//! the behavior in
//! `fixtures/tasks_mail/send/F-SEND.bs4_normalize.golden.json`.
//!
//! Mention spans are collected during the same pass (output coordinates),
//! so `<mention-component>`-lookalikes inside raw text, comments, PIs,
//! CDATA sections or attribute values are never elements — exactly like
//! `soup.find_all("mention-component")`.
//!
//! Known unavoidable divergence: lone surrogates (`&#xD800;`-`&#xDFFF;`
//! in text) stay surrogates in CPython's output but map to U+FFFD here —
//! a Rust `String` cannot hold a surrogate. The golden records U+FFFD for
//! those cases with this note as the justification.
//!
//! Known minor divergence: CPython's `re.I` close scan finds non-ASCII
//! case variants (`</ſcript>`, U+017F) and keeps them as content, while
//! this ASCII-only scan misses them and drops the tail instead
//! (`<script>x</ſcript>TAIL` keeps `x</ſcript>` there, drops all here).
//! Corpus-absent; ASCII closes and all recorded behavior are unaffected.

use pidash_types::html_entities::{c1_remap, html_unescape, lookup_bare_entity};

/// What [`normalize_bs`] found: the normalized HTML plus every
/// `<mention-component>` span in output coordinates, document order
/// (outer before inner, like `find_all`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    /// `str(BeautifulSoup(html, "html.parser"))`.
    pub html: String,
    /// Mention spans in [`Normalized::html`] coordinates, document order.
    pub mentions: Vec<NormalizedMention>,
}

/// One `<mention-component>` element: its span in normalized coordinates
/// plus the decoded `entity_identifier` (`None` when absent — the Python
/// `KeyError` case).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedMention {
    /// Byte offset of the `<` opening the element.
    pub start: usize,
    /// Byte offset just past the element (through `</mention-component>`).
    pub end: usize,
    /// Decoded `entity_identifier` attribute value.
    pub entity_id: Option<String>,
}

/// Normalize `html` like `str(BeautifulSoup(html, "html.parser"))` and
/// collect the `<mention-component>` spans (`soup.find_all` order).
pub fn normalize_bs(html: &str) -> Normalized {
    let mut p = Parser::new(html);
    p.run();
    Normalized {
        html: p.out,
        mentions: p.mentions,
    }
}

/// ASCII whitespace per the HTML spec (`\x0b` excluded — probed).
fn is_html_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

/// First byte of an HTML tag or entity name: ASCII letters only
/// (`html.parser` uses `[a-zA-Z]`).
fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic()
}

/// Void elements (`<x/>` on output): the HTML void set plus the obsolete
/// empties bs4 keeps (`keygen`, `command`, `menuitem`, `basefont`,
/// `bgsound`, `frame`, `image`, `isindex`, `nextid`, `spacer` — probed).
fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "basefont"
            | "bgsound"
            | "br"
            | "col"
            | "command"
            | "embed"
            | "frame"
            | "hr"
            | "img"
            | "input"
            | "isindex"
            | "keygen"
            | "link"
            | "menuitem"
            | "meta"
            | "nextid"
            | "param"
            | "source"
            | "spacer"
            | "track"
            | "wbr"
            | "image"
    )
}

/// Raw-text elements (`CDATA_CONTENT_ELEMENTS` on 3.12.10): no tags
/// are parsed inside; content runs verbatim to the matching close and
/// is dropped entirely when no close exists (probed).
fn is_raw(name: &str) -> bool {
    matches!(name, "script" | "style")
}

/// Python `re` `\s` for `str` patterns (probed on 3.12.10): ASCII
/// whitespace plus `\x0b`, `\x1c`-`\x1f`, and Unicode White_Space
/// (`\x85`, `\xa0`, `\u1680`, `\u2000`-`\u200a`, `\u2028`-`\u2029`,
/// `\u202f`, `\u205f`, `\u3000`). Rust `char::is_whitespace` lacks
/// only `\x1c`-`\x1f`; `\u180e` and `\ufeff` are excluded on both.
fn is_py_space(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}

/// Byte length of the Python-space char at `bytes[k]` (0 when the byte
/// at `k` starts no space; callers only pass char boundaries).
fn py_space_len(bytes: &[u8], k: usize) -> usize {
    let b = bytes[k];
    if b < 0x80 {
        return usize::from(matches!(
            b,
            b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c | 0x1c | 0x1d | 0x1e | 0x1f
        ));
    }
    let s = std::str::from_utf8(&bytes[k..]).unwrap_or("");
    match s.chars().next() {
        Some(c) if c.is_whitespace() => c.len_utf8(),
        _ => 0,
    }
}

/// Skip Python spaces from `k` (`\s*`).
fn skip_py_spaces(bytes: &[u8], mut k: usize) -> usize {
    while k < bytes.len() {
        let n = py_space_len(bytes, k);
        if n == 0 {
            break;
        }
        k += n;
    }
    k
}

/// Skip one `(?:\s|/(?!>))*` run from `k` (callers pass a char
/// boundary; the result is one too).
fn skip_tag_sep(bytes: &[u8], mut k: usize) -> usize {
    while k < bytes.len() {
        let n = py_space_len(bytes, k);
        if n > 0 {
            k += n;
            continue;
        }
        if bytes[k] == b'/' && !(k + 1 < bytes.len() && bytes[k + 1] == b'>') {
            k += 1;
            continue;
        }
        break;
    }
    k
}

/// Scan a bare attr value (`[^>\s]*`) from `v` (a boundary).
fn skip_bare_value(bytes: &[u8], mut v: usize) -> usize {
    while v < bytes.len() {
        let b = bytes[v];
        if b == b'>' {
            break;
        }
        if b < 0x80 {
            if py_space_len(bytes, v) > 0 {
                break;
            }
            v += 1;
        } else {
            match std::str::from_utf8(&bytes[v..])
                .unwrap_or("")
                .chars()
                .next()
            {
                Some(c) if !c.is_whitespace() => {
                    v += c.len_utf8();
                }
                _ => break,
            }
        }
    }
    v
}

/// The optional `=value` group after an attr name ending at `m`:
/// `(value, w)`. A present `=` almost always wins (probed):
/// quoted values scan unbounded for their closer; when none exists
/// the post-`=` spaces backtrack to an empty bare value (`a= "y`
/// keeps `a=""` and `"y` becomes the next attr); else one `=`
/// joins a bare value (`a==="y` keeps `="y`); only a single `=`
/// with no spaces drops the group. `None` (with `w = m`) means no
/// `=` followed, or that dropped single-`=` shape.
fn scan_attr_value(bytes: &[u8], m: usize) -> (Option<String>, usize) {
    let len = bytes.len();
    let mut v = skip_py_spaces(bytes, m);
    if v >= len || bytes[v] != b'=' {
        return (None, m);
    }
    let eq = v;
    v += 1;
    while v < len && bytes[v] == b'=' {
        v += 1;
    }
    let val_start = skip_py_spaces(bytes, v);
    let post_spaces = val_start - v;
    v = val_start;
    if v < len && (bytes[v] == b'"' || bytes[v] == b'\'') {
        let q = bytes[v];
        let mut n = v + 1;
        while n < len && bytes[n] != q {
            n += 1;
        }
        if n < len {
            return (
                Some(html_unescape(
                    std::str::from_utf8(&bytes[v + 1..n]).unwrap_or(""),
                )),
                n + 1,
            );
        }
        if post_spaces > 0 {
            return (Some(String::new()), v);
        }
        if v - eq > 1 {
            // One `=` rejoins as a bare value (`a==="y`).
            let n = skip_bare_value(bytes, v - 1);
            return (
                Some(html_unescape(
                    std::str::from_utf8(&bytes[v - 1..n]).unwrap_or(""),
                )),
                n,
            );
        }
        return (None, m);
    }
    let n = skip_bare_value(bytes, v);
    (
        Some(html_unescape(
            std::str::from_utf8(&bytes[v..n]).unwrap_or(""),
        )),
        n,
    )
}

/// Char ending at byte `k` (`0 < k <= len`, boundary) for the
/// attrfind lookbehind.
fn char_before(bytes: &[u8], k: usize) -> char {
    debug_assert!(k > 0 && k <= bytes.len());
    if bytes[k - 1] < 0x80 {
        return bytes[k - 1] as char;
    }
    let mut s = k - 1;
    while s > 0 && bytes[s] & 0xC0 == 0x80 {
        s -= 1;
    }
    std::str::from_utf8(&bytes[s..k])
        .unwrap_or("")
        .chars()
        .next()
        .unwrap_or('\u{fffd}')
}

/// Whitespace-preserving elements: the ws-collapse never applies inside
/// their subtree (probed, any depth).
fn is_pre(name: &str) -> bool {
    matches!(name, "pre" | "textarea")
}

/// Text-path numeric value (`html.parser` greedy rule, probed):
/// C1 remapped, NUL kept, huge → U+FFFD. Surrogates cannot live in a
/// Rust `String`, so they map to U+FFFD (Python keeps the lone
/// surrogate; unavoidable divergence, one char class).
fn text_numeric(n: u64) -> char {
    if (0x80..=0x9f).contains(&n) {
        return c1_remap(n as u32);
    }
    if n > 0x10ffff {
        return '\u{fffd}';
    }
    char::from_u32(n as u32).unwrap_or('\u{fffd}')
}

/// Escape text exactly like bs4 serializes a text node: only `&`, `<`,
/// `>` (`'` and `"` stay literal — probed).
fn escape_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

/// Collapse a whitespace-only chunk: `\n` when it contains one,
/// else a single space (probed).
fn collapse_ws(chunk: &str) -> &str {
    if chunk.contains('\n') {
        "\n"
    } else {
        " "
    }
}

/// Whether a `parse_*` consumed a construct (classic `goahead`: a
/// -1 with `end` set falls back to the data slice below).
use MarkupFlow::{Consumed, Incomplete};

/// One open element on the builder stack.
struct OpenElement {
    /// Lowercased tag name.
    name: String,
    /// Index into `mentions` when this is a `mention-component`.
    mention: Option<usize>,
    /// Whether this element preserves whitespace (`pre`/`textarea`).
    pre: bool,
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    out: String,
    /// Pending text chunk (merged across literal `&`/`<` and decoded
    /// entities; flushed at markup boundaries and EOF — probed).
    pending: String,
    stack: Vec<OpenElement>,
    /// Count of `pre`/`textarea` on the stack (ws-collapse guard).
    pre_depth: usize,
    mentions: Vec<NormalizedMention>,
    /// Invalid-charref arming (probed CPython quirk): the first `&#`
    /// that fails the `charref` match (non-digit tail, or digits with
    /// a hex-letter terminator) arms this; the next match failure
    /// detonates (remainder raw to EOF). A regex match always decodes
    /// and never consults this. Global to the input — tags and valid
    /// refs never reset it. Attributes never consult it
    /// (`html.unescape` is stateless). Mechanically this is bs4's
    /// feed-then-close split: the first bad ref is consumed by `feed`
    /// and parsing resumes in `close`, while the second breaks `close`
    /// and flushes the rest as data.
    charref_armed: bool,
    /// A markup construct already hit the EOF data rule. Classic
    /// `feed` stops at the first incomplete construct, so an invalid
    /// `&#` after one is processed by `close` and detonates at once
    /// (probed) instead of arming.
    saw_markup_break: bool,
}

/// What a markup parse decided (classic `goahead` dispatch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkupFlow {
    /// A construct was consumed (`self.pos` advanced past it).
    Consumed,
    /// No complete construct (`parse_*` returned -1): the caller emits
    /// the EOF data slice. Nothing was flushed or advanced.
    Incomplete,
}

impl<'a> Parser<'a> {
    fn new(html: &'a str) -> Self {
        Parser {
            bytes: html.as_bytes(),
            pos: 0,
            out: String::with_capacity(html.len()),
            pending: String::new(),
            stack: Vec::new(),
            pre_depth: 0,
            mentions: Vec::new(),
            charref_armed: false,
            saw_markup_break: false,
        }
    }

    fn rest(&self) -> &'a [u8] {
        &self.bytes[self.pos..]
    }

    fn starts_with(&self, s: &[u8]) -> bool {
        self.rest().starts_with(s)
    }

    /// Byte substring as `str` (callers only cut at ASCII bytes or
    /// recorded char boundaries, so this cannot fail).
    fn slice(&self, from: usize, to: usize) -> &'a str {
        std::str::from_utf8(&self.bytes[from..to]).unwrap_or("")
    }

    /// Char starting at byte `k` (`None` at EOF; callers pass char
    /// boundaries).
    fn char_at(&self, k: usize) -> Option<char> {
        if k >= self.bytes.len() {
            return None;
        }
        self.slice(k, self.bytes.len()).chars().next()
    }

    fn run(&mut self) {
        while self.pos < self.bytes.len() {
            if self.bytes[self.pos] == b'<' {
                if !self.parse_markup() {
                    // `<` that opens nothing is literal text (it merges
                    // into the pending chunk — probed).
                    self.pending.push('<');
                    self.pos += 1;
                }
            } else {
                self.parse_text();
            }
        }
        self.flush_text();
        self.close_all_at_eof();
    }

    /// Flush the pending text chunk: collapse when whitespace-only and
    /// outside `<pre>`/`<textarea>`, then escape into the output.
    fn flush_text(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let chunk = std::mem::take(&mut self.pending);
        if self.pre_depth == 0 && chunk.bytes().all(is_html_ws) {
            self.out.push_str(collapse_ws(&chunk));
        } else {
            escape_text(&chunk, &mut self.out);
        }
    }

    /// Scan text to the next `<` or EOF, decoding entities inline (they
    /// merge into the pending chunk — probed).
    fn parse_text(&mut self) {
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == b'<' {
                return;
            }
            if b == b'&' {
                if self.entity_text() == EntityFlow::Break {
                    return;
                }
                continue;
            }
            let c = self
                .slice(self.pos, self.bytes.len())
                .chars()
                .next()
                .unwrap_or('\0');
            self.pending.push(c);
            self.pos += c.len_utf8().max(1);
        }
    }

    /// The `&#`-without-digits break (probed CPython quirk): with no `;`
    /// anywhere later, `&` goes literal and the whole remainder turns to
    /// raw text (no tags, no entities), merged into the pending chunk.
    fn raw_rest(&mut self) {
        self.pending.push('&');
        self.pos += 1;
        self.pending
            .push_str(self.slice(self.pos, self.bytes.len()));
        self.pos = self.bytes.len();
    }

    /// One text-path `&...` at `self.pos` (which is `&`).
    fn entity_text(&mut self) -> EntityFlow {
        let len = self.bytes.len();
        if self.pos + 1 >= len {
            self.pending.push('&');
            self.pos += 1;
            return EntityFlow::Normal;
        }
        let next = self.bytes[self.pos + 1];
        if next == b'#' {
            return self.numeric_text();
        }
        if !is_name_start(next) {
            self.pending.push('&');
            self.pos += 1;
            return EntityFlow::Normal;
        }
        // Greedy `[a-zA-Z][-.a-zA-Z0-9]*`, single lookup, no
        // backtracking (probed: `&ampb` stays literal).
        let mut k = self.pos + 2;
        while k < len
            && (self.bytes[k].is_ascii_alphanumeric()
                || self.bytes[k] == b'-'
                || self.bytes[k] == b'.')
        {
            k += 1;
        }
        if k >= len {
            // No terminator at EOF: `&` + one letter drops the `&`
            // (the `incomplete`-at-EOF rule — probed: `<p>a&x` →
            // `<p>ax</p>`); longer tails stay literal (probed: `&ab`,
            // `&x-`). The attribute path never does this.
            if k > self.pos + 2 {
                self.pending.push('&');
            }
            self.pos += 1;
            return EntityFlow::Normal;
        }
        let name = self.slice(self.pos + 1, k);
        let semi = self.bytes[k] == b';';
        match lookup_bare_entity(name) {
            Some(v) => {
                self.pending.push_str(v);
            }
            None => {
                // Miss: literal `&` + name; a `;` terminator is consumed
                // with it (probed: `&unknown;` → `&amp;unknown`).
                self.pending.push('&');
                self.pending.push_str(name);
            }
        }
        self.pos = k + usize::from(semi);
        EntityFlow::Normal
    }

    /// One text-path `&#...` at `self.pos` (probed shape table):
    /// - `x`/`X` + hex digits: decoded whatever terminates it (a hex
    ///   ref never arms, never detonates, never breaks).
    /// - digits: `;`-terminated decodes; a hex-letter terminator
    ///   defeats the `charref` regex, so it takes the invalid-ref
    ///   path; any other terminator is a regex match and always
    ///   decodes, armed or not (arming only governs invalid refs).
    /// - anything else: the classic break when no `;` follows
    ///   anywhere (remainder raw to EOF); otherwise arms when fresh,
    ///   detonates when armed, then literal `&`.
    fn numeric_text(&mut self) -> EntityFlow {
        let len = self.bytes.len();
        let mut k = self.pos + 2;
        if k < len && (self.bytes[k] == b'x' || self.bytes[k] == b'X') {
            k += 1;
            let digits = k;
            let mut n: u64 = 0;
            while k < len && self.bytes[k].is_ascii_hexdigit() {
                let d = u64::from(
                    self.bytes[k].to_ascii_lowercase()
                        - if self.bytes[k].is_ascii_digit() {
                            b'0'
                        } else {
                            b'a' - 10
                        },
                );
                n = n.saturating_mul(16).saturating_add(d);
                k += 1;
            }
            if k == digits {
                return self.hash_break();
            }
            if k >= len {
                self.pending.push('&');
                self.pos += 1;
                return EntityFlow::Normal;
            }
            let semi = self.bytes[k] == b';';
            self.pending.push(text_numeric(n));
            self.pos = k + usize::from(semi);
            return EntityFlow::Normal;
        }
        if k < len && self.bytes[k].is_ascii_digit() {
            let mut n: u64 = 0;
            while k < len && self.bytes[k].is_ascii_digit() {
                n = n
                    .saturating_mul(10)
                    .saturating_add(u64::from(self.bytes[k] - b'0'));
                k += 1;
            }
            if k < len && self.bytes[k] == b';' {
                self.pending.push(text_numeric(n));
                self.pos = k + 1;
                return EntityFlow::Normal;
            }
            // A hex-letter terminator defeats the `charref` regex
            // (`[^0-9a-fA-F]` required after digits), so this is an
            // invalid ref: arm/break/detonate (probed: `&#65b;c`
            // stays literal, while `&#65g;` decodes).
            if k < len && matches!(self.bytes[k], b'a'..=b'f' | b'A'..=b'F') {
                return self.hash_break();
            }
            // Any other terminator is a regex match, which always
            // decodes — arming only governs invalid refs (probed).
            if k >= len {
                self.pending.push('&');
                self.pos += 1;
                return EntityFlow::Normal;
            }
            self.pending.push(text_numeric(n));
            self.pos = k;
            return EntityFlow::Normal;
        }
        self.hash_break()
    }

    /// `&#` that fails the `charref` match (not followed by digits,
    /// or digits with a hex-letter terminator): the classic break
    /// when no `;` appears anywhere later (remainder raw to EOF);
    /// otherwise arm when fresh, detonate when armed (or after a
    /// markup break, which already spent `feed`'s pass), then literal
    /// `&` (probed).
    fn hash_break(&mut self) -> EntityFlow {
        if !self.bytes[self.pos + 1..].contains(&b';') {
            self.raw_rest();
            return EntityFlow::Break;
        }
        if self.charref_armed || self.saw_markup_break {
            self.raw_rest();
            return EntityFlow::Break;
        }
        self.charref_armed = true;
        self.pending.push('&');
        self.pos += 1;
        EntityFlow::Normal
    }
}

/// Whether text scanning continues normally after an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntityFlow {
    Normal,
    /// The `&#` break fired: remainder is raw, input exhausted.
    Break,
}

/// Quote an attribute value exactly like bs4: single quotes when the
/// value contains `"` but not `'`, else double quotes with `"` escaped
/// (probed). `&`, `<`, `>` always escaped; `'` stays literal.
fn quote_attr(value: &str, out: &mut String) {
    if value.contains('"') && !value.contains('\'') {
        out.push('\'');
        for c in value.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                _ => out.push(c),
            }
        }
        out.push('\'');
    } else {
        out.push('"');
        for c in value.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                _ => out.push(c),
            }
        }
        out.push('"');
    }
}

impl<'a> Parser<'a> {
    /// Parse markup at `self.pos` (which is `<`). Returns whether a
    /// markup construct was consumed (`false` = literal `<` text,
    /// merged into the pending chunk by the caller).
    fn parse_markup(&mut self) -> bool {
        let len = self.bytes.len();
        if self.pos + 1 >= len {
            return false;
        }
        let start = self.pos;
        let next = self.bytes[start + 1];
        let flow = if is_name_start(next) {
            self.parse_open_tag(start)
        } else if next == b'/' {
            self.parse_end_tag(start)
        } else if self.starts_with(b"<!--") {
            self.parse_comment(start)
        } else if next == b'?' {
            self.parse_pi(start)
        } else if next == b'!' {
            self.parse_declaration(start)
        } else {
            return false;
        };
        if flow == Incomplete {
            self.eof_data(start);
        }
        true
    }

    /// Classic `goahead` with `end` set and a -1: data runs to the
    /// next `>` past the `<`, else to the next `<`, else one char
    /// (probed). The slice merges into the pending chunk — bs4 sees
    /// one data run, so no flush happens here.
    fn eof_data(&mut self, start: usize) {
        let after = start + 1;
        let rest = &self.bytes[after..];
        let k = rest
            .iter()
            .position(|&b| b == b'>')
            .map(|p| after + p + 1)
            .or_else(|| rest.iter().position(|&b| b == b'<').map(|p| after + p))
            .unwrap_or(after);
        self.pending.push_str(self.slice(start, k));
        self.pos = k;
        self.saw_markup_break = true;
    }

    /// `<!` followed by the 7-letter `doctype` word (case-insensitive,
    /// no word boundary — probed: `<!doctypeX>` is a doctype).
    fn is_doctype_at(&self, start: usize) -> bool {
        self.bytes.len() >= start + 9
            && self.bytes[start..start + 9].eq_ignore_ascii_case(b"<!doctype")
    }

    /// Parse an open tag at `start` (`<` + letter). An incomplete
    /// tag reports [`Incomplete`][] (the caller applies the EOF data
    /// rule); when the attrs do not consume to `>`/`/>` the tag span
    /// itself becomes data (probed).
    fn parse_open_tag(&mut self, start: usize) -> MarkupFlow {
        let len = self.bytes.len();
        // `tagfind_tolerant` group 1: ASCII letter, then anything but
        // HTML space, `/`, `>` or NUL (`\x0b` and multibyte included).
        let mut j = start + 2;
        while j < len
            && !matches!(
                self.bytes[j],
                b'\t' | b'\n' | 0x0c | b'\r' | b' ' | b'/' | b'>' | 0x00
            )
        {
            j += 1;
        }
        // Tag names lowercase with Unicode `lower` (Python `str.lower`).
        let name = self.slice(start + 1, j).to_lowercase();
        let Some(endpos) = self.check_for_whole_start_tag(start, j) else {
            return Incomplete;
        };
        let mut k = skip_tag_sep(self.bytes, j);
        let mut attrs = Vec::new();
        while k < endpos {
            let Some(((attr_name, attr_value), next)) = self.scan_attr(k) else {
                break;
            };
            attrs.push((attr_name, attr_value));
            k = next;
        }
        // `end = rawdata[k:endpos].strip()` (empty when the attr scan
        // ran past the tag end — slicing clamps).
        let end = if k > endpos {
            ""
        } else {
            self.slice(k, endpos).trim_matches(is_py_space)
        };
        if end != ">" && end != "/>" {
            // Attrs did not consume the tag: the source span becomes
            // data (no markup break — `feed` never stopped here).
            self.pending.push_str(self.slice(start, endpos));
            self.pos = endpos;
            return Consumed;
        }
        self.flush_text();
        self.pos = endpos;
        let start_out = self.out.len();
        self.out.push('<');
        self.out.push_str(&name);
        emit_attrs(&attrs, &mut self.out);
        if end == "/>" {
            // XHTML-style empty tag: start immediately followed by end.
            if is_void(&name) {
                self.out.push_str("/>");
            } else {
                self.out.push('>');
                self.out.push_str("</");
                self.out.push_str(&name);
                self.out.push('>');
                if name == "mention-component" {
                    self.record_mention(start_out, self.out.len(), &attrs);
                }
            }
            return Consumed;
        }
        if is_void(&name) {
            self.out.push_str("/>");
            return Consumed;
        }
        self.out.push('>');
        let mention = if name == "mention-component" {
            Some(self.claim_mention(start_out, &attrs))
        } else {
            None
        };
        let pre = is_pre(&name);
        self.pre_depth += usize::from(pre);
        self.stack.push(OpenElement {
            name: name.clone(),
            mention,
            pre,
        });
        if is_raw(&name) {
            self.parse_rawtext(&name);
        }
        Consumed
    }

    /// `check_for_whole_start_tag`: the `locatestarttagend_tolerant`
    /// scan from the tag name end. Returns the byte past `>`/`/>`, or
    /// `None` when the tag is incomplete (buffer boundary, or a name
    /// char where the end was expected — probed). Shape: one `[\s/]*`
    /// run, then attr iterations (separator backtracking needs no
    /// emulation: attr-start chars and separators are disjoint, so
    /// greedy skips are deterministic).
    fn check_for_whole_start_tag(&self, start: usize, mut k: usize) -> Option<usize> {
        let len = self.bytes.len();
        // `[\s/]*` once, before the attribute iterations.
        while k < len {
            let n = py_space_len(self.bytes, k);
            if n > 0 {
                k += n;
                continue;
            }
            if self.bytes[k] == b'/' {
                k += 1;
                continue;
            }
            break;
        }
        // Attribute iterations (each requires a name).
        while let Some(first) = self.char_at(k) {
            let prev = char_before(self.bytes, k);
            if !(prev == '"' || prev == '\'' || prev == '/' || is_py_space(prev))
                || is_py_space(first)
                || first == '/'
                || first == '>'
            {
                break;
            }
            let mut m = k + first.len_utf8();
            while m < len {
                match self.char_at(m) {
                    Some(c) if !is_py_space(c) && !matches!(c, '/' | '=' | '>') => {
                        m += c.len_utf8();
                    }
                    _ => break,
                }
            }
            let (_, w) = scan_attr_value(self.bytes, m);
            k = skip_tag_sep(self.bytes, w);
        }
        let k = skip_py_spaces(self.bytes, k);
        if k >= len {
            return None;
        }
        let nb = self.bytes[k];
        if nb == b'>' {
            return Some(k + 1);
        }
        if nb == b'/' {
            if self.bytes[k..].starts_with(b"/>") {
                return Some(k + 2);
            }
            // A `/` that is not `/>` here never completes (buffer
            // boundary in CPython terms).
            return None;
        }
        if nb.is_ascii_alphabetic() || nb == b'=' || nb == b'/' {
            // End of input in or before an attribute value.
            return None;
        }
        if k > start {
            Some(k)
        } else {
            Some(start + 1)
        }
    }

    /// One `attrfind_tolerant` match at `k`: the attribute plus the end
    /// position, or `None` (the loop breaks with `k` unchanged).
    /// Quoted values scan unbounded for their closer, past `>` and to
    /// EOF; `=` repeats collapse (`a==b` keeps `b` — probed).
    fn scan_attr(&self, k: usize) -> Option<((String, String), usize)> {
        let len = self.bytes.len();
        if k == 0 || k >= len {
            return None;
        }
        // Lookbehind `(?<=['"\s/])`.
        let prev = char_before(self.bytes, k);
        if !(prev == '"' || prev == '\'' || prev == '/' || is_py_space(prev)) {
            return None;
        }
        // Name: `[^\s/>]` + `[^\s/=>]*` (quotes, `=`, `<`, NUL all
        // allowed inside — probed).
        let first = self.char_at(k)?;
        if is_py_space(first) || first == '/' || first == '>' {
            return None;
        }
        let mut m = k + first.len_utf8();
        while m < len {
            match self.char_at(m) {
                Some(c) if !is_py_space(c) && !matches!(c, '/' | '=' | '>') => {
                    m += c.len_utf8();
                }
                _ => break,
            }
        }
        let attr_name = self.slice(k, m).to_lowercase();
        let (value, w) = scan_attr_value(self.bytes, m);
        let w = skip_tag_sep(self.bytes, w);
        Some(((attr_name, value.unwrap_or_default()), w))
    }

    /// Parse an end tag at `start` (any `</`). `endendtag` finds
    /// the first `>` past it (none: [`Incomplete`][]); a clean
    /// `</name>` (spaces allowed around the name — probed) closes,
    /// else the `tagfind` name closes at the first `>` past the name
    /// (junk is skipped whole, quotes included — probed); `</>` drops
    /// silently; anything else is a bogus comment. Closes stack
    /// elements through the match; unmatched names are dropped.
    fn parse_end_tag(&mut self, start: usize) -> MarkupFlow {
        let len = self.bytes.len();
        let Some(rel) = self.bytes[start + 1..].iter().position(|&b| b == b'>') else {
            return Incomplete;
        };
        let gtpos = start + 1 + rel + 1;
        // Clean close: `endtagfind` = `</\s*([a-zA-Z][-.a-zA-Z0-9:_]*)\s*>`.
        let k = skip_py_spaces(self.bytes, start + 2);
        if k < len && self.bytes[k].is_ascii_alphabetic() {
            let mut m = k + 1;
            while m < len
                && matches!(self.bytes[m], b'-' | b'.' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b':' | b'_')
            {
                m += 1;
            }
            if skip_py_spaces(self.bytes, m) + 1 == gtpos {
                let name = self.slice(k, m).to_lowercase();
                self.flush_text();
                self.pos = gtpos;
                self.close_through(&name);
                return Consumed;
            }
        }
        // Fallback: the `tagfind_tolerant` name closes at the first
        // `>` past it (`</p=foo>` never matches the stack, so it
        // drops — probed).
        if start + 2 < len && is_name_start(self.bytes[start + 2]) {
            let mut j = start + 3;
            while j < len
                && !matches!(
                    self.bytes[j],
                    b'\t' | b'\n' | 0x0c | b'\r' | b' ' | b'/' | b'>' | 0x00
                )
            {
                j += 1;
            }
            let name = self.slice(start + 2, j).to_lowercase();
            let end = self.bytes[j..]
                .iter()
                .position(|&b| b == b'>')
                .map(|p| j + p + 1)
                .unwrap_or(len);
            self.flush_text();
            self.pos = end;
            self.close_through(&name);
            return Consumed;
        }
        if self.bytes[start..].starts_with(b"</>") {
            // Dropped silently (probed) — not even a chunk split.
            self.pos = start + 3;
            return Consumed;
        }
        self.parse_bogus_inner(start)
    }

    /// Pop stack elements through `name`, emitting each close (probed
    /// misnesting: `</b>` in `<b><i>` emits `</i></b>`).
    fn close_through(&mut self, name: &str) {
        if let Some(idx) = self.stack.iter().rposition(|e| e.name == *name) {
            while self.stack.len() > idx {
                let elem = self.stack.pop().expect("idx in range");
                self.pre_depth -= usize::from(elem.pre);
                self.out.push_str("</");
                self.out.push_str(&elem.name);
                self.out.push('>');
                if let Some(mi) = elem.mention {
                    self.mentions[mi].end = self.out.len();
                }
            }
        }
    }

    /// Record a self-closed mention span.
    fn record_mention(&mut self, start: usize, end: usize, attrs: &[(String, String)]) {
        self.mentions.push(NormalizedMention {
            start,
            end,
            entity_id: entity_identifier(attrs),
        });
    }

    /// Claim a span for a pushed mention element (end filled on close).
    fn claim_mention(&mut self, start: usize, attrs: &[(String, String)]) -> usize {
        let idx = self.mentions.len();
        self.mentions.push(NormalizedMention {
            start,
            end: start,
            entity_id: entity_identifier(attrs),
        });
        idx
    }

    fn close_all_at_eof(&mut self) {
        while let Some(elem) = self.stack.pop() {
            self.pre_depth -= usize::from(elem.pre);
            self.out.push_str("</");
            self.out.push_str(&elem.name);
            self.out.push('>');
            if let Some(mi) = elem.mention {
                self.mentions[mi].end = self.out.len();
            }
        }
    }
}

/// The decoded `entity_identifier` (last wins — attr dup rule).
fn entity_identifier(attrs: &[(String, String)]) -> Option<String> {
    attrs
        .iter()
        .rev()
        .find(|(name, _)| name == "entity_identifier")
        .map(|(_, value)| value.clone())
}

/// Emit attributes sorted byte-wise, last-wins on duplicates (probed).
fn emit_attrs(attrs: &[(String, String)], out: &mut String) {
    let mut sorted: Vec<(usize, &(String, String))> = attrs.iter().enumerate().collect();
    sorted.sort_by(|a, b| a.1 .0.as_bytes().cmp(b.1 .0.as_bytes()).then(a.0.cmp(&b.0)));
    let mut last: Option<&str> = None;
    // Walk from the back so the last duplicate wins, then reverse.
    let mut picked: Vec<&(String, String)> = Vec::with_capacity(attrs.len());
    for (_, attr) in sorted.iter().rev() {
        if last != Some(attr.0.as_str()) {
            last = Some(attr.0.as_str());
            picked.push(attr);
        }
    }
    for (name, value) in picked.iter().rev() {
        out.push(' ');
        out.push_str(name);
        out.push('=');
        quote_attr(value, out);
    }
}

impl<'a> Parser<'a> {
    /// Emit opaque content (comment/CDATA/bogus bodies): collapsed when
    /// whitespace-only outside `<pre>`/`<textarea>`, empty → single
    /// space (probed).
    fn emit_opaque(&mut self, content: &str) {
        if content.is_empty() {
            self.out.push(' ');
        } else if self.pre_depth == 0 && content.bytes().all(is_html_ws) {
            self.out.push_str(collapse_ws(content));
        } else {
            self.out.push_str(content);
        }
    }

    /// `<!--...-->`: ends at the first `-->` or `-- >`
    /// (`commentclose = --\s*>` — probed); `--!>` never closes and
    /// there is no abrupt close, so `<!-->`/`<!--a--!>` without a
    /// later closer report [`Incomplete`][] and become text.
    fn parse_comment(&mut self, start: usize) -> MarkupFlow {
        let body = start + 4;
        let Some((content_end, after)) = find_comment_close(self.bytes, body) else {
            return Incomplete;
        };
        self.flush_text();
        self.out.push_str("<!--");
        self.emit_opaque(self.slice(body, content_end));
        self.out.push_str("-->");
        self.pos = after;
        Consumed
    }

    /// `<!...>` dispatch (`parse_html_declaration`): `<!--` (already
    /// handled by the caller, kept for the shared shape), `<![` marked
    /// sections, `<!doctype` (9-char case-insensitive prefix, no word
    /// boundary — probed), else bogus comment.
    fn parse_declaration(&mut self, start: usize) -> MarkupFlow {
        if self.bytes[start..].starts_with(b"<![") {
            return self.parse_marked_section(start);
        }
        if self.is_doctype_at(start) {
            return self.parse_doctype(start);
        }
        self.parse_bogus_inner(start)
    }

    /// `<![...]>` marked section (`_markupbase`): the decl name decides
    /// the closer (`cdata`/`temp`/`ignore`/`include`/`rcdata` take
    /// `]\s*]\s*>`, `if`/`else`/`endif` take `]\s*>`). CDATA renders
    /// as `<![CDATA[...]]>`, the rest as `Declaration` (`<?...?>` —
    /// bs4's `PREFIX`/`SUFFIX`). Unknown names and non-name content
    /// raise `AssertionError` in CPython (bs4 refuses the markup), so
    /// they have no portable output; treat them as bogus (probed:
    /// unreachable from the corpus, which never raises).
    fn parse_marked_section(&mut self, start: usize) -> MarkupFlow {
        // `_declname_match` = `[a-zA-Z][-_.a-zA-Z0-9]*\s*`.
        let mut k = start + 3;
        if k >= self.bytes.len() || !is_name_start(self.bytes[k]) {
            return self.parse_bogus_inner(start);
        }
        k += 1;
        while k < self.bytes.len()
            && matches!(self.bytes[k], b'-' | b'_' | b'.' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9')
        {
            k += 1;
        }
        let name = self.slice(start + 3, k).to_lowercase();
        k = skip_py_spaces(self.bytes, k);
        if k >= self.bytes.len() {
            // A name running to EOF is incomplete (`_scan_name`
            // returns -1).
            return Incomplete;
        }
        let ms_close = matches!(name.as_str(), "if" | "else" | "endif");
        if !ms_close
            && !matches!(
                name.as_str(),
                "temp" | "cdata" | "ignore" | "include" | "rcdata"
            )
        {
            return self.parse_bogus_inner(start);
        }
        let body = start + 3;
        let found = if ms_close {
            find_ms_close(self.bytes, body)
        } else {
            find_marked_close(self.bytes, body)
        };
        let Some((content_end, after)) = found else {
            return Incomplete;
        };
        let mut content = self.slice(body, content_end);
        self.flush_text();
        // `unknown_decl`: `CDATA[`-prefixed content (case-insensitive)
        // is a CDATA section, the rest a declaration.
        if content.len() >= 6 && content.as_bytes()[..6].eq_ignore_ascii_case(b"CDATA[") {
            content = &content[6..];
            self.out.push_str("<![CDATA[");
            self.emit_opaque(content);
            self.out.push_str("]]>");
        } else {
            self.out.push_str("<?");
            self.out.push_str(content);
            self.out.push_str("?>");
        }
        self.pos = after;
        Consumed
    }

    /// `<?...?>`: ends at the first `>` (none: [`Incomplete`][]),
    /// content verbatim, never ws-collapsed, empty → single space
    /// (probed).
    fn parse_pi(&mut self, start: usize) -> MarkupFlow {
        let body = start + 2;
        let Some(rel) = self.bytes[body..].iter().position(|&b| b == b'>') else {
            return Incomplete;
        };
        self.flush_text();
        self.out.push_str("<?");
        let data = self.slice(body, body + rel);
        self.out.push_str(if data.is_empty() { " " } else { data });
        self.out.push('>');
        self.pos = body + rel + 1;
        Consumed
    }

    /// `<!DOCTYPE...>` (and case variants): keyword uppercased, the
    /// first 8 data chars (`doctype` + one) stripped, `>\n` appended
    /// (probed). No `>`: [`Incomplete`][].
    fn parse_doctype(&mut self, start: usize) -> MarkupFlow {
        let body = start + 2;
        let search_from = start + 9;
        let Some(rel) = self.bytes[search_from..].iter().position(|&b| b == b'>') else {
            return Incomplete;
        };
        let end = search_from + rel;
        let data = self.slice(body, end);
        // Python strips 8 *characters* (`doctype` + one); cut at the
        // 8th char boundary (byte 8 can split a multibyte char).
        let rest = data
            .char_indices()
            .nth(8)
            .map(|(i, _)| &data[i..])
            .unwrap_or("");
        self.flush_text();
        self.out.push_str("<!DOCTYPE ");
        self.out.push_str(rest);
        if rest.is_empty() {
            self.out.push(' ');
        }
        self.out.push_str(">\n");
        self.pos = end + 1;
        Consumed
    }

    /// Bogus comment (`<!...>` that is not a comment/marked
    /// section/doctype, or `</` + non-letter): content to the first
    /// `>` (none: [`Incomplete`][] — probed).
    fn parse_bogus_inner(&mut self, start: usize) -> MarkupFlow {
        let body = start + 2;
        let Some(rel) = self.bytes[body..].iter().position(|&b| b == b'>') else {
            return Incomplete;
        };
        self.flush_text();
        self.out.push_str("<!--");
        self.emit_opaque(self.slice(body, body + rel));
        self.out.push_str("-->");
        self.pos = body + rel + 1;
        Consumed
    }

    /// Raw-text content for the just-pushed script/style element:
    /// the `interesting` regex (`</\s*name\s*>`, case-insensitive)
    /// finds the close anywhere later; the content runs verbatim to
    /// it. With no close anywhere, `feed` and `close` both stall and
    /// the whole rest is dropped (probed: `<script>a` renders
    /// `<script></script>`).
    fn parse_rawtext(&mut self, name: &str) {
        let len = self.bytes.len();
        let content_start = self.pos;
        let mut k = content_start;
        let mut found: Option<(usize, usize)> = None;
        while k < len {
            if self.bytes[k] == b'<' {
                if let Some(after) = match_cdata_close(self.bytes, k, name) {
                    found = Some((k, after));
                    break;
                }
            }
            k += 1;
        }
        match found {
            Some((close_at, after)) => {
                self.emit_rawtext(self.slice(content_start, close_at));
                self.pos = after;
                self.pop_raw();
            }
            None => {
                self.pos = len;
                self.pop_raw();
            }
        }
    }

    /// Emit raw-text content verbatim (collapsed when whitespace-only
    /// outside `<pre>`/`<textarea>` — probed).
    fn emit_rawtext(&mut self, content: &str) {
        if content.is_empty() {
            // No output for empty raw text.
        } else if self.pre_depth == 0 && content.bytes().all(is_html_ws) {
            self.out.push_str(collapse_ws(content));
        } else {
            self.out.push_str(content);
        }
    }

    /// Pop the raw element itself, emitting its close (called with the
    /// element on top of the stack).
    fn pop_raw(&mut self) {
        if let Some(elem) = self.stack.pop() {
            self.pre_depth -= usize::from(elem.pre);
            self.out.push_str("</");
            self.out.push_str(&elem.name);
            self.out.push('>');
            debug_assert!(elem.mention.is_none(), "raw elements are never mentions");
        }
    }
}

/// `</\s*name\s*>` at `k` (the `interesting` regex in cdata
/// mode), case-insensitive: the byte past `>`, or `None` (probed:
/// `</script foo>` and `</script/>` never close).
fn match_cdata_close(bytes: &[u8], k: usize, name: &str) -> Option<usize> {
    if !bytes[k..].starts_with(b"</") {
        return None;
    }
    let mut m = skip_py_spaces(bytes, k + 2);
    let nb = name.as_bytes();
    if bytes.len() < m + nb.len() || !bytes[m..m + nb.len()].eq_ignore_ascii_case(nb) {
        return None;
    }
    m = skip_py_spaces(bytes, m + nb.len());
    if m < bytes.len() && bytes[m] == b'>' {
        Some(m + 1)
    } else {
        None
    }
}

/// First `--\s*>` at or past `from`: `(content_end, after)`.
fn find_comment_close(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut k = from;
    while k + 1 < bytes.len() {
        if bytes[k] == b'-' && bytes[k + 1] == b'-' {
            let m = skip_py_spaces(bytes, k + 2);
            if m < bytes.len() && bytes[m] == b'>' {
                return Some((k, m + 1));
            }
        }
        k += 1;
    }
    None
}

/// First `]\s*]\s*>` at or past `from`: `(content_end, after)`.
fn find_marked_close(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut k = from;
    while k < bytes.len() {
        if bytes[k] == b']' {
            let m = skip_py_spaces(bytes, k + 1);
            if m < bytes.len() && bytes[m] == b']' {
                let n = skip_py_spaces(bytes, m + 1);
                if n < bytes.len() && bytes[n] == b'>' {
                    return Some((k, n + 1));
                }
            }
        }
        k += 1;
    }
    None
}

/// First `]\s*>` at or past `from`: `(content_end, after)` (MS Office
/// `if`/`else`/`endif` sections).
fn find_ms_close(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut k = from;
    while k < bytes.len() {
        if bytes[k] == b']' {
            let m = skip_py_spaces(bytes, k + 1);
            if m < bytes.len() && bytes[m] == b'>' {
                return Some((k, m + 1));
            }
        }
        k += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(html: &str) -> String {
        normalize_bs(html).html
    }

    /// The recording interpreter pinned by PIDASHCONV-1693: the
    /// golden asserts production's parser (3.12.10, classic
    /// `html.parser`), and the test refuses a golden recorded
    /// anywhere else so an oracle skew fails loudly instead of
    /// silently re-pinning behavior.
    const GOLDEN_PYTHON: &str = "3.12.10";
    const GOLDEN_BS4: &str = "4.12.3";

    #[test]
    fn golden_bs4_normalize() {
        let raw =
            include_str!("../../../../fixtures/tasks_mail/send/F-SEND.bs4_normalize.golden.json");
        let fixture: serde_json::Value = serde_json::from_str(raw).expect("golden fixture parses");
        assert_eq!(
            fixture["_python"].as_str().unwrap_or("?"),
            GOLDEN_PYTHON,
            "golden recorded on the wrong interpreter"
        );
        assert_eq!(
            fixture["_bs4"].as_str().unwrap_or("?"),
            GOLDEN_BS4,
            "golden recorded against the wrong bs4"
        );
        let cases = fixture["cases"].as_array().expect("cases array");
        assert!(!cases.is_empty(), "golden fixture has no cases");
        let mut failures: Vec<String> = Vec::new();
        for case in cases {
            let name = case["name"].as_str().unwrap_or("?");
            let input = case["in"].as_str().unwrap_or("");
            let expected = case["out"].as_str().unwrap_or("");
            let got = norm(input);
            if got != expected {
                failures.push(format!(
                    "{name}\n  in:       {input:?}\n  expected:  {expected:?}\n  got:       {got:?}"
                ));
                if failures.len() >= 15 {
                    break;
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} golden case(s) differ (showing up to 15):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn issue_repro_normalizes_entities_and_voids() {
        let html = "<p>a&nbsp;&nbsp;b<br>c <img src=\"x.png\"> it&#39;s &quot;q&quot; \
            <mention-component entity_identifier=\"u1\" label=\"x\"></mention-component></p>";
        assert_eq!(
            norm(html),
            "<p>a\u{a0}\u{a0}b<br/>c <img src=\"x.png\"/> it\'s \"q\" \
            <mention-component entity_identifier=\"u1\" label=\"x\"></mention-component></p>"
        );
    }

    #[test]
    fn non_ascii_around_name_boundary() {
        // PIDASHCONV-1284: the old `rest[..17]` slicing panicked when
        // byte 17 after `<` fell inside a multi-byte char. The bs4
        // engine scans bytes, so every width at every offset just
        // parses (no panic is the assertion); the name shapes still
        // classify right.
        for ch in ['é', '’', '😀'] {
            for offset in 0..=17 {
                let rest = format!("{}{ch}{}", "a".repeat(offset), "b".repeat(20));
                let _ = normalize_bs(&format!("<{rest}>x"));
                let _ = normalize_bs(&format!("</{rest}>x"));
            }
            // `<mention-componenté…>` is not a tag (é is no delimiter).
            let n = normalize_bs(&format!("<mention-component{ch} tail>x"));
            assert!(n.mentions.is_empty(), "char {ch:?}");
            // `</mention-componenté>` never closes the real element.
            let n = normalize_bs(&format!("<p>x</mention-component{ch}>tail"));
            assert_eq!(n.html, "<p>xtail</p>", "char {ch:?}");
        }
        // Positive control: the real tag still matches.
        let n = normalize_bs("<mention-component entity_identifier=\"u1\">x</mention-component>");
        assert_eq!(n.mentions.len(), 1);
    }

    #[test]
    fn decimal_ref_with_hex_letter_terminator_stays_literal() {
        // The `charref` regex needs `[^0-9a-fA-F]` after digits.
        assert_eq!(norm("<p>&#65b;c</p>"), "<p>&amp;#65b;c</p>");
        assert_eq!(norm("<p>&#65B;c</p>"), "<p>&amp;#65B;c</p>");
        assert_eq!(norm("<p>&#65b</p>"), "<p>&amp;#65b&lt;/p&gt;</p>");
        // Other-letter terminators still decode.
        assert_eq!(norm("<p>&#65g;z</p>"), "<p>Ag;z</p>");
        assert_eq!(norm("<p>&#65x;z</p>"), "<p>Ax;z</p>");
    }

    #[test]
    fn decodable_refs_decode_even_when_armed() {
        assert_eq!(norm("<p>a&#x; b &#65 c;d</p>"), "<p>a&amp;#x; b A c;d</p>");
        assert_eq!(norm("<p>&#x; &#x41;</p>"), "<p>&amp;#x; A</p>");
        // …but a hex-letter terminator detonates when armed.
        assert_eq!(
            norm("<p>&#x; &#66b;c</p>"),
            "<p>&amp;#x; &amp;#66b;c&lt;/p&gt;</p>"
        );
    }

    #[test]
    fn endtag_junk_closes_at_first_gt() {
        // Junk is skipped whole to the first `>` past the name —
        // quotes (bare or `name="..."`) never protect it.
        assert_eq!(norm("<p>x</p \"a>b\">tail"), "<p>x</p>b\"&gt;tail");
        assert_eq!(norm("<script>x</script \"a>b\">tail"), "<script></script>");
        assert_eq!(
            norm("<title>x</title \"a>b\">tail"),
            "<title>x</title>b\"&gt;tail"
        );
        assert_eq!(norm("<p>x</p a=\">\">tail"), "<p>x</p>\"&gt;tail");
        assert_eq!(norm("<p>x</p a=\"y>TAIL"), "<p>x</p>TAIL");
        assert_eq!(norm("<p>x</p a =\"y>TAIL"), "<p>x</p>TAIL");
        // `=` and `/` start junk attrs.
        assert_eq!(norm("<p>x</p =>tail"), "<p>x</p>tail");
        assert_eq!(norm("<p>x</p/x>tail"), "<p>x</p>tail");
    }

    #[test]
    fn endtag_junk_with_quotes_closes_at_inner_gt() {
        // Post-`=` ws, quote style, and tab ws change nothing: the
        // close still lands on the inner `>`.
        assert_eq!(norm("<p>x</p a= \"y>TAIL"), "<p>x</p>TAIL");
        assert_eq!(norm("<p>x</p a= 'y>TAIL"), "<p>x</p>TAIL");
        assert_eq!(norm("<p>x</p a=  \"y>TAIL"), "<p>x</p>TAIL");
        assert_eq!(norm("<p>x</p a=\t\"y>TAIL"), "<p>x</p>TAIL");
        // A junky would-be close inside script content is content: the
        // later clean close wins and keeps everything verbatim.
        assert_eq!(
            norm("<script>x</script a= \"y>TAIL</script>END"),
            "<script>x</script a= \"y>TAIL</script>END"
        );
        assert_eq!(
            norm("<title>x</title a= \"y>TAIL</title>END"),
            "<title>x</title>TAILEND"
        );
        // Controls: terminated and `\x0b` shapes close; the EOF shape
        // (no `>` at all) becomes text.
        assert_eq!(norm("<p>x</p a= \"b\">TAIL"), "<p>x</p>TAIL");
        assert_eq!(norm("<p>x</p a= \"y"), "<p>x&lt;/p a= \"y</p>");
        assert_eq!(norm("<p>x</p a=\x0b\"y>TAIL"), "<p>x</p>TAIL");
    }

    #[test]
    fn open_tag_backtracks_post_eq_ws_before_unterminated_quote() {
        // `a` keeps `=` + empty-bare (`a=""`), then `"y` parses as a
        // bare attrname and the tag completes (attrs render sorted).
        assert_eq!(norm("<p a= \"y>z</p>"), "<p \"y=\"\" a=\"\">z</p>");
        assert_eq!(norm("<p a= 'y>z</p>"), "<p 'y=\"\" a=\"\">z</p>");
        assert_eq!(norm("<p a=  \"y>z</p>"), "<p \"y=\"\" a=\"\">z</p>");
        assert_eq!(norm("<p a=\t\"y>z</p>"), "<p \"y=\"\" a=\"\">z</p>");
        // Controls: no-ws and EOF shapes become text; pre-ws-only and
        // terminated shapes are unchanged; `\x0b` is a separator, so
        // it backtracks like a space.
        assert_eq!(norm("<p a=\"y>z</p>"), "&lt;p a=\"y&gt;z");
        assert_eq!(norm("<p a =\"y>z</p>"), "<p =\"y=\"\" a=\"\">z</p>");
        assert_eq!(norm("<p a= \"b\">z</p>"), "<p a=\"b\">z</p>");
        assert_eq!(norm("<p a= \"y"), "&lt;p a= \"y");
        assert_eq!(norm("<p a=\x0b\"y>z</p>"), "<p \"y=\"\" a=\"\">z</p>");
        // No post-`=` ws: the value group matches empty, so the scan
        // resumes after the attrname past attrfind's lookbehind — a
        // skipped pre-`=` sep or a quote-final name completes the tag.
        assert_eq!(norm("<p a\"=\"y>z</p>"), "<p =\"y=\"\" a\"=\"\">z</p>");
        assert_eq!(norm("<p a\" =\"y>z</p>"), "<p =\"y=\"\" a\"=\"\">z</p>");
        assert_eq!(norm("<p ab =\"y>z</p>"), "<p =\"y=\"\" ab=\"\">z</p>");
        assert_eq!(norm("<p ab=\"y>z</p>"), "&lt;p ab=\"y&gt;z");
        assert_eq!(norm("<p a\t=\"y>z</p>"), "<p =\"y=\"\" a=\"\">z</p>");
        assert_eq!(
            norm("<p b=\"1\" a =\"y>z</p>"),
            "<p =\"y=\"\" a=\"\" b=\"1\">z</p>"
        );
        assert_eq!(norm("<p a =\"y"), "&lt;p a =\"y");
    }

    #[test]
    fn raw_close_needs_clean_close_or_content_drops() {
        // Junky would-be closes never match `</name\s*>`, so the
        // whole rest drops for script/style.
        assert_eq!(norm("<script>x</script <b"), "<script></script>");
        assert_eq!(norm("<style>x</style <b"), "<style></style>");
        // title/textarea/iframe are normal elements: the dangling
        // end tag becomes text instead.
        assert_eq!(
            norm("<title>x</title <b"),
            "<title>x&lt;/title &lt;b</title>"
        );
        assert_eq!(
            norm("<textarea>x</textarea <b"),
            "<textarea>x&lt;/textarea &lt;b</textarea>"
        );
        assert_eq!(
            norm("<iframe>x</iframe <b"),
            "<iframe>x&lt;/iframe &lt;b</iframe>"
        );
        // `</name` at EOF drops the content the same way.
        assert_eq!(norm("<script>x</script"), "<script></script>");
        assert_eq!(norm("<title>x</title"), "<title>x&lt;/title</title>");
        // A later clean close wins mid-input and keeps everything
        // before it verbatim.
        assert_eq!(
            norm("<script>x</script <b>y</script>TAIL"),
            "<script>x</script <b>y</script>TAIL"
        );
    }

    #[test]
    fn single_letter_entity_at_eof_drops_ampersand() {
        assert_eq!(norm("<p>a&x"), "<p>ax</p>");
        assert_eq!(norm("<title>a&x"), "<title>ax</title>");
        assert_eq!(norm("<p>a&ab"), "<p>a&amp;ab</p>");
        assert_eq!(norm("<p>a&x-"), "<p>a&amp;x-</p>");
        assert_eq!(norm("<p>a&"), "<p>a&amp;</p>");
        assert_eq!(norm("<p>a&#"), "<p>a&amp;#</p>");
    }

    #[test]
    fn nul_is_not_a_tag_name_char() {
        // NUL ends the open-tag name scan (the tag then fails its
        // end check and becomes text), while end tags still match
        // the name before it.
        assert_eq!(norm("<p\x00>x</p>"), "&lt;p\x00&gt;x");
        assert_eq!(norm("<p>x</p\x00>tail"), "<p>x</p>tail");
        assert_eq!(norm("<p\x00>x</p\x00>tail"), "&lt;p\x00&gt;xtail");
    }

    #[test]
    fn dash_bang_gt_does_not_close_comments() {
        // `--!>` is not a closer (`commentclose` is `--\s*>`), so a
        // `--!>` without a later `-->` becomes text; with one, the
        // `--!>` stays inside the comment.
        assert_eq!(norm("<!--x--!>"), "&lt;!--x--!&gt;");
        assert_eq!(norm("<!--a--!>b-->"), "<!--a--!>b-->");
        assert_eq!(norm("<!--a--!>TAIL"), "&lt;!--a--!&gt;TAIL");
        assert_eq!(norm("<!--x-->TAIL"), "<!--x-->TAIL");
    }

    #[test]
    fn unterminated_comment_at_eof_becomes_text() {
        // No closer anywhere: the EOF data rule emits the whole
        // construct as text (no abrupt close, no suffix strip).
        assert_eq!(norm("<!--a--"), "&lt;!--a--");
        assert_eq!(norm("<!--a-"), "&lt;!--a-");
        assert_eq!(norm("<!--a--!"), "&lt;!--a--!");
        assert_eq!(norm("<!--a---"), "&lt;!--a---");
        assert_eq!(norm("<!-----"), "&lt;!-----");
        assert_eq!(norm("<!---"), "&lt;!---");
        assert_eq!(norm("<!--a--x"), "&lt;!--a--x");
        assert_eq!(norm("<!--a-- "), "&lt;!--a-- ");
        assert_eq!(norm("<!--a-- x"), "&lt;!--a-- x");
        assert_eq!(norm("<!--!"), "&lt;!--!");
        assert_eq!(norm("<!--a"), "&lt;!--a");
        assert_eq!(norm("<!--"), "&lt;!--");
    }

    #[test]
    fn equals_in_endtag_names_drops() {
        assert_eq!(norm("<p>x</p=foo>tail"), "<p>xtail</p>");
        assert_eq!(norm("<p>x</p=foo>"), "<p>x</p>");
        assert_eq!(norm("<p>x</p=>tail"), "<p>xtail</p>");
    }

    #[test]
    fn mention_spans_cover_elements_in_order() {
        let n = normalize_bs(
            "<p>a<mention-component entity_identifier=\"u1\">x</mention-component> \
            b<mention-component entity_identifier=\"u2\"/>c</p>",
        );
        assert_eq!(
            n.html,
            "<p>a<mention-component entity_identifier=\"u1\">x</mention-component> \
            b<mention-component entity_identifier=\"u2\"></mention-component>c</p>"
        );
        assert_eq!(n.mentions.len(), 2);
        assert_eq!(n.mentions[0].entity_id.as_deref(), Some("u1"));
        assert_eq!(n.mentions[1].entity_id.as_deref(), Some("u2"));
        assert_eq!(
            &n.html[n.mentions[0].start..n.mentions[0].end],
            "<mention-component entity_identifier=\"u1\">x</mention-component>"
        );
        assert_eq!(
            &n.html[n.mentions[1].start..n.mentions[1].end],
            "<mention-component entity_identifier=\"u2\"></mention-component>"
        );
    }

    #[test]
    fn mention_missing_attribute_records_none() {
        let n = normalize_bs("<mention-component>oops</mention-component>");
        assert_eq!(n.mentions.len(), 1);
        assert_eq!(n.mentions[0].entity_id, None);
    }

    #[test]
    fn mention_entity_identifier_is_decoded() {
        // bs4 hands Python the decoded attr value (`mention[…]`).
        let n = normalize_bs("<mention-component entity_identifier=\"a&amp;b\"/>");
        assert_eq!(n.mentions[0].entity_id.as_deref(), Some("a&b"));
    }

    #[test]
    fn mention_lookalikes_outside_elements_are_not_spans() {
        // Raw text keeps `<mention-component…>` verbatim but it is not
        // an element (Python `find_all` finds nothing there).
        let n = normalize_bs("<script><mention-component entity_identifier=\"u1\"/></script>");
        assert!(n.mentions.is_empty());
        let n = normalize_bs("<!--<mention-component entity_identifier=\"u1\"/>-->");
        assert!(n.mentions.is_empty());
        let n =
            normalize_bs("<p title=\"<mention-component entity_identifier=&#34;u1&#34;>\">x</p>");
        assert!(n.mentions.is_empty());
    }

    #[test]
    fn mention_inside_title_is_a_span() {
        // `title` is a normal element on 3.12.10 (no RCDATA), so an
        // inner mention is a real element (probed).
        let n = normalize_bs("<title><mention-component entity_identifier=\"u1\"/></title>");
        assert_eq!(
            n.html,
            "<title><mention-component entity_identifier=\"u1\"></mention-component></title>"
        );
        assert_eq!(n.mentions.len(), 1);
        assert_eq!(n.mentions[0].entity_id.as_deref(), Some("u1"));
    }

    #[test]
    fn nested_mentions_both_recorded_outer_first() {
        let n = normalize_bs("<mention-component entity_identifier=\"o\">a<mention-component entity_identifier=\"i\">b</mention-component>c</mention-component>");
        assert_eq!(n.mentions.len(), 2);
        assert_eq!(n.mentions[0].entity_id.as_deref(), Some("o"));
        assert_eq!(n.mentions[1].entity_id.as_deref(), Some("i"));
        assert!(n.mentions[0].start < n.mentions[1].start);
        assert!(n.mentions[1].end < n.mentions[0].end);
    }

    #[test]
    fn text_escapes_only_amp_lt_gt() {
        assert_eq!(norm("<p>'\"</p>"), "<p>'\"</p>");
        assert_eq!(norm("<p>&</p>"), "<p>&amp;</p>");
    }

    #[test]
    fn attr_quoting_prefers_double_unless_dquote_present() {
        assert_eq!(norm("<p title=\"a'b\">x</p>"), "<p title=\"a'b\">x</p>");
        assert_eq!(norm("<p title=\"a&quot;b\">x</p>"), "<p title='a\"b'>x</p>");
        assert_eq!(
            norm("<p title=\"a'b&quot;c\">x</p>"),
            "<p title=\"a'b&quot;c\">x</p>"
        );
    }

    #[test]
    fn attrs_sorted_last_wins() {
        assert_eq!(
            norm("<p z=\"1\" a=\"2\" m=\"3\">x</p>"),
            "<p a=\"2\" m=\"3\" z=\"1\">x</p>"
        );
        assert_eq!(norm("<p a=\"1\" a=\"2\">x</p>"), "<p a=\"2\">x</p>");
    }

    #[test]
    fn text_numeric_keeps_nul_but_remaps_c1() {
        assert_eq!(norm("<p>&#0;</p>"), "<p>\x00</p>");
        assert_eq!(norm("<p>&#128;</p>"), "<p>€</p>");
        assert_eq!(norm("<p>&#x110000;</p>"), "<p>\u{fffd}</p>");
        // Surrogates cannot live in a Rust String (Python keeps them).
        assert_eq!(norm("<p>&#xD800;</p>"), "<p>\u{fffd}</p>");
    }
}
