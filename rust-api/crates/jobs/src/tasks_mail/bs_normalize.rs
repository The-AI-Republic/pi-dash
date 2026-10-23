//! `str(BeautifulSoup(html, "html.parser"))`, ported from CPython 3.12 /
//! bs4 4.12.3 (`email_notification_task.py:130-140` runs every mention
//! content string through it, even with no `mention-component` in it).
//!
//! [`normalize_bs`] parses `html` exactly like `html.parser` (greedy
//! no-backtrack entity refs in text, `html.unescape` in attributes, void
//! elements, script/style/title/textarea/xmp/iframe/noembed/noframes/
//! plaintext raw text, bogus comments, abrupt comment closes,
//! whitespace-only collapsing outside `<pre>`/`<textarea>`) and
//! serializes exactly like bs4 (entities decoded then minimally
//! re-escaped, void elements as `<x/>`, attributes lowercased, sorted,
//! double-quoted). Every rule is probe-verified; the full probe corpus
//! plus a whole-table sweep pins the behavior in
//! `fixtures/tasks_mail/send/F-SEND.bs4_normalize.golden.json`.
//!
//! Mention spans are collected during the same pass (output coordinates),
//! so `<mention-component>`-lookalikes inside raw text, comments, PIs,
//! CDATA sections or attribute values are never elements — exactly like
//! `soup.find_all("mention-component")`.

use std::cmp::Ordering;

use super::bs_entities_generated::{ATTR_INVALID_CODEPOINTS, C1_MAP, HTML5_ENTITIES};

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

/// Raw-text elements: no tags are parsed inside; closed only by the
/// matching end tag (`plaintext` never closes — probed).
fn is_raw(name: &str) -> bool {
    matches!(
        name,
        "script"
            | "style"
            | "title"
            | "textarea"
            | "xmp"
            | "iframe"
            | "noembed"
            | "noframes"
            | "plaintext"
    )
}

/// RCDATA elements: entities decoded, tags-as-text (probed).
fn is_rcdata(name: &str) -> bool {
    matches!(name, "title" | "textarea")
}

/// Verbatim raw elements (script/style): content emitted byte-for-byte.
/// All other raw elements escape `&`, `<`, `>` on output (xmp, iframe,
/// noembed, noframes and plaintext escape without decoding; title and
/// textarea decode then escape — probed).
fn is_verbatim_raw(name: &str) -> bool {
    matches!(name, "script" | "style")
}

/// Whitespace-preserving elements: the ws-collapse never applies inside
/// their subtree (probed, any depth).
fn is_pre(name: &str) -> bool {
    matches!(name, "pre" | "textarea")
}

/// Look up an entity `name` (exact key, with or without `;`).
fn lookup_entity(name: &str) -> Option<&'static str> {
    HTML5_ENTITIES
        .binary_search_by(|probe| probe.0.as_bytes().cmp(name.as_bytes()))
        .ok()
        .map(|idx| HTML5_ENTITIES[idx].1)
}

/// Look up `name`, then `name;` (probed fallback: `&notin<x` → `∉`),
/// without allocating the `;` form.
fn lookup_entity_then_semi(name: &str) -> Option<&'static str> {
    if let Some(value) = lookup_entity(name) {
        return Some(value);
    }
    let want = name.as_bytes();
    // Binary search for the target `name;`, comparing each key against
    // the unmaterialized target (`want` + one `;` byte).
    let found = HTML5_ENTITIES
        .binary_search_by(|probe| {
            let key = probe.0.as_bytes();
            let shared = key.len().min(want.len());
            match key[..shared].cmp(&want[..shared]) {
                Ordering::Equal => {
                    if shared < want.len() {
                        // Key is a strict prefix of `name`: shorter than
                        // the target.
                        Ordering::Less
                    } else if key.len() == want.len() {
                        // Key equals `name`: shorter than `name;`.
                        Ordering::Less
                    } else {
                        // `name` is a strict prefix of the key: compare
                        // the next key byte against `;`.
                        match key[want.len()].cmp(&b';') {
                            Ordering::Equal => key.len().cmp(&(want.len() + 1)),
                            ord => ord,
                        }
                    }
                }
                ord => ord,
            }
        })
        .ok()?;
    Some(HTML5_ENTITIES[found].1)
}

/// C1 control remap shared by both decoders (`html._invalid_charrefs`).
fn c1_remap(n: u32) -> char {
    C1_MAP[(n - 0x80) as usize]
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

/// Attribute-path numeric value (`html.unescape`, exact):
/// `_invalid_charrefs` first, then `_invalid_codepoints` → empty,
/// then surrogates/range → U+FFFD. `None` is the empty case.
fn attr_numeric(n: u64) -> Option<char> {
    if n == 0 {
        return Some('\u{fffd}');
    }
    if n == 13 {
        return Some('\r');
    }
    if (0x80..=0x9f).contains(&n) {
        return Some(c1_remap(n as u32));
    }
    if n <= 0x10ffff && ATTR_INVALID_CODEPOINTS.binary_search(&(n as u32)).is_ok() {
        return None;
    }
    if n > 0x10ffff {
        return Some('\u{fffd}');
    }
    Some(char::from_u32(n as u32).unwrap_or('\u{fffd}'))
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

/// Parsed attributes: `(attrs, end, selfclose)`.
type ParsedAttrs = (Vec<(String, String)>, usize, bool);

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
    /// (`html.unescape` is stateless).
    charref_armed: bool,
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
        match lookup_entity_then_semi(name) {
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
    /// otherwise arm when fresh, detonate when armed, then literal
    /// `&` (probed).
    fn hash_break(&mut self) -> EntityFlow {
        if !self.bytes[self.pos + 1..].contains(&b';') {
            self.raw_rest();
            return EntityFlow::Break;
        }
        if self.charref_armed {
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

/// `html.unescape`, exact port (attribute values — probed): greedy
/// numerics with optional `;`, named longest-prefix with backtracking
/// (`&copyx;` → `©x;`), no terminator requirement, invalid numerics via
/// the CPython tables.
fn unescape_attr(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'&' {
            let c = s[i..].chars().next().unwrap_or('\0');
            out.push(c);
            i += c.len_utf8().max(1);
            continue;
        }
        if let Some((value, end)) = attr_numeric_at(bytes, i) {
            if let Some(c) = value {
                out.push(c);
            }
            i = end;
            continue;
        }
        // Named span: `[^\t\n\f <&#;]{1,32}` + optional `;`
        // (`\r` allowed, `\x0b` allowed — the exclusion set is exact).
        let mut k = i + 1;
        let mut chars = 0;
        while k < bytes.len()
            && chars < 32
            && !matches!(
                bytes[k],
                b'\t' | b'\n' | 0x0c | b' ' | b'<' | b'&' | b'#' | b';'
            )
        {
            let c = s[k..].chars().next().unwrap_or('\0');
            k += c.len_utf8().max(1);
            chars += 1;
        }
        if k < bytes.len() && bytes[k] == b';' {
            k += 1;
        }
        if k == i + 1 {
            out.push('&');
            i += 1;
            continue;
        }
        let span = &s[i + 1..k];
        // Longest prefix that is a table key (exact-with-`;` wins when
        // longest: `&notin;` → `∉`, while `&notit;` → `¬it;`).
        let mut hit: Option<(&str, &str)> = None;
        let mut end = span.len();
        while end > 0 {
            while end > 0 && !span.is_char_boundary(end) {
                end -= 1;
            }
            if end == 0 {
                break;
            }
            if let Some(value) = lookup_entity(&span[..end]) {
                hit = Some((value, &span[end..]));
                break;
            }
            end -= 1;
        }
        match hit {
            Some((value, rest)) => {
                out.push_str(value);
                out.push_str(rest);
                i = k;
            }
            None => {
                out.push('&');
                i += 1;
            }
        }
    }
    out
}

/// Greedy attr numeric at `&`: `#[0-9]+;?` or `#[xX][hex]+;?`.
/// The `Option<char>` is `None` for the empty-string invalids.
fn attr_numeric_at(bytes: &[u8], i: usize) -> Option<(Option<char>, usize)> {
    if i + 2 > bytes.len() || bytes[i] != b'&' || bytes[i + 1] != b'#' {
        return None;
    }
    let mut k = i + 2;
    let hex = k < bytes.len() && (bytes[k] == b'x' || bytes[k] == b'X');
    if hex {
        k += 1;
    }
    let digits = k;
    let mut n: u64 = 0;
    if hex {
        while k < bytes.len() && bytes[k].is_ascii_hexdigit() {
            let d = u64::from(
                bytes[k].to_ascii_lowercase()
                    - if bytes[k].is_ascii_digit() {
                        b'0'
                    } else {
                        b'a' - 10
                    },
            );
            n = n.saturating_mul(16).saturating_add(d);
            k += 1;
        }
    } else {
        while k < bytes.len() && bytes[k].is_ascii_digit() {
            n = n
                .saturating_mul(10)
                .saturating_add(u64::from(bytes[k] - b'0'));
            k += 1;
        }
    }
    if k == digits {
        return None;
    }
    if k < bytes.len() && bytes[k] == b';' {
        k += 1;
    }
    Some((attr_numeric(n), k))
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
    /// markup construct was consumed (`false` = literal `<` text).
    fn parse_markup(&mut self) -> bool {
        let len = self.bytes.len();
        if self.pos + 1 >= len {
            return false;
        }
        let next = self.bytes[self.pos + 1];
        if is_name_start(next) {
            self.flush_text();
            self.parse_open_tag();
            return true;
        }
        if self.starts_with(b"<!--") {
            self.flush_text();
            self.parse_comment();
            return true;
        }
        if self.starts_with(b"<![CDATA[") {
            self.flush_text();
            self.parse_cdata();
            return true;
        }
        if self.is_doctype() {
            self.flush_text();
            self.parse_doctype();
            return true;
        }
        if next == b'!' {
            self.flush_text();
            self.parse_bogus(self.pos + 2);
            return true;
        }
        if next == b'/' {
            if self.pos + 2 < len && is_name_start(self.bytes[self.pos + 2]) {
                self.flush_text();
                self.parse_end_tag();
                return true;
            }
            if self.starts_with(b"</>") {
                // Dropped silently (probed) — not even a chunk split.
                self.pos += 3;
                return true;
            }
            if self.pos + 2 >= len {
                return false;
            }
            self.flush_text();
            self.parse_bogus(self.pos + 2);
            return true;
        }
        if next == b'?' {
            self.flush_text();
            self.parse_pi();
            return true;
        }
        false
    }

    /// `<!` followed by the 7-letter `doctype` word (case-insensitive,
    /// no word boundary — probed: `<!doctypeX>` is a doctype).
    fn is_doctype(&self) -> bool {
        self.rest().len() >= 9 && self.rest()[2..9].eq_ignore_ascii_case(b"doctype")
    }

    /// Parse an open tag at `self.pos`. An incomplete tag (no `>`,
    /// unterminated quote) drops the whole remainder (probed).
    fn parse_open_tag(&mut self) {
        let len = self.bytes.len();
        let mut j = self.pos + 2;
        // `tagfind_tolerant` group 1: NUL is a name char (probed).
        while j < len
            && !matches!(
                self.bytes[j],
                b'\t' | b'\n' | 0x0c | b'\r' | b' ' | b'/' | b'>'
            )
        {
            j += 1;
        }
        // Tag names lowercase with Unicode `lower` (Python `str.lower`).
        let name = self.slice(self.pos + 1, j).to_lowercase();
        let Some((attrs, end, selfclose)) = self.parse_attrs(j) else {
            self.pos = len;
            return;
        };
        self.pos = end;
        let start = self.out.len();
        self.out.push('<');
        self.out.push_str(&name);
        emit_attrs(&attrs, &mut self.out);
        if selfclose {
            if is_void(&name) {
                self.out.push_str("/>");
            } else {
                self.out.push('>');
                self.out.push_str("</");
                self.out.push_str(&name);
                self.out.push('>');
                if name == "mention-component" {
                    self.record_mention(start, self.out.len(), &attrs);
                }
            }
            return;
        }
        if is_void(&name) {
            self.out.push_str("/>");
            return;
        }
        self.out.push('>');
        let mention = if name == "mention-component" {
            Some(self.claim_mention(start, &attrs))
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
            self.parse_raw(&name);
        }
    }

    /// Parse attributes from `j`: `(attrs, end, selfclose)`, or `None`
    /// for an incomplete tag (drops the remainder — probed).
    fn parse_attrs(&self, mut k: usize) -> Option<ParsedAttrs> {
        let len = self.bytes.len();
        let mut attrs = Vec::new();
        loop {
            // Separators: whitespace plus `/` not immediately before
            // `>` (probed: `<p a/b>`, `<p /a>`, `<p / >`).
            while k < len
                && (is_html_ws(self.bytes[k])
                    || (self.bytes[k] == b'/' && !(k + 1 < len && self.bytes[k + 1] == b'>')))
            {
                k += 1;
            }
            if k >= len {
                return None;
            }
            if self.bytes[k] == b'>' {
                return Some((attrs, k + 1, false));
            }
            if self.bytes[k] == b'/' && k + 1 < len && self.bytes[k + 1] == b'>' {
                return Some((attrs, k + 2, true));
            }
            // Name: one `[^ws/>]` char (quotes, `=`, `<` allowed —
            // probed) plus `[^ws/=>]*`.
            let mut m = k + 1;
            while m < len
                && !is_html_ws(self.bytes[m])
                && !matches!(self.bytes[m], b'=' | b'>' | b'/')
            {
                m += 1;
            }
            let attr_name = self.slice(k, m).to_lowercase();
            let after_name = m;
            k = m;
            while k < len && is_html_ws(self.bytes[k]) {
                k += 1;
            }
            let mut value = String::new();
            if k < len && self.bytes[k] == b'=' {
                let eq_pos = k;
                k += 1;
                while k < len && is_html_ws(self.bytes[k]) {
                    k += 1;
                }
                if k >= len {
                    return None;
                }
                let q = self.bytes[k];
                if q == b'"' || q == b'\'' {
                    let mut n = k + 1;
                    while n < len && self.bytes[n] != q {
                        n += 1;
                    }
                    if n >= len {
                        // Unterminated quote. With post-`=` ws
                        // consumed, CPython backtracks the ws to empty
                        // and keeps `=` + empty-bare (`a=""`),
                        // resuming after the `=` (probed: `<p a= "y>`
                        // completes). With no post-`=` ws the value
                        // group matches empty and the scan resumes
                        // after the attrname — but only past
                        // attrfind's lookbehind, i.e. a skipped sep
                        // (pre-`=` ws: `eq_pos > after_name`) or a
                        // quote-final name (probed: `<p a ="y>` and
                        // `<p a"="y>` complete, `<p a="y>` drops the
                        // tag); otherwise the tag is incomplete.
                        if k > eq_pos + 1 {
                            k = eq_pos + 1;
                        } else if eq_pos > after_name
                            || matches!(self.bytes[after_name - 1], b'"' | b'\'')
                        {
                            k = after_name;
                        } else {
                            return None;
                        }
                    } else {
                        value = unescape_attr(self.slice(k + 1, n));
                        k = n + 1;
                    }
                } else {
                    // Bare: everything to whitespace or `>` (quotes and
                    // `/` included — probed: `a=b'c'`, `value=1/`).
                    let mut n = k;
                    while n < len && !is_html_ws(self.bytes[n]) && self.bytes[n] != b'>' {
                        n += 1;
                    }
                    value = unescape_attr(self.slice(k, n));
                    k = n;
                }
            }
            attrs.push((attr_name, value));
        }
    }

    /// Parse an end tag at `self.pos` (`</` + letter). The name is
    /// `tagfind_tolerant` group 1 (`=`, `<`, quotes and NUL included —
    /// probed: `</p=foo>`, `</p<x>`, `</p"x">` never match the stack,
    /// so they are dropped); junk after it scans attrfind-shaped to
    /// `>` (a scan that never reaches `>` swallows to EOF, where the
    /// auto-close is byte-identical — so normal tags complete either
    /// way). Closes stack elements through the match; unmatched names
    /// are dropped.
    fn parse_end_tag(&mut self) {
        let mut j = self.pos + 3;
        let len = self.bytes.len();
        while j < len && !is_html_ws(self.bytes[j]) && !matches!(self.bytes[j], b'/' | b'>') {
            j += 1;
        }
        let name = self.slice(self.pos + 2, j).to_lowercase();
        self.pos = scan_endtag_junk(self.bytes, j).unwrap_or(len);
        // Pop through the match, emitting each close (probed
        // misnesting: `</b>` in `<b><i>` emits `</i></b>`).
        if let Some(idx) = self.stack.iter().rposition(|e| e.name == name) {
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

    /// `<!--...-->`: ends at the first `-->` or `--!>`
    /// (`commentclose = --!?>` — probed), whichever comes first. With
    /// neither later, `<!-->`/`<!--->` close abruptly (empty);
    /// otherwise the content runs to EOF minus one trailing `--!`,
    /// `--` or `-` (probed).
    fn parse_comment(&mut self) {
        let len = self.bytes.len();
        let body = self.pos + 4;
        let tail = &self.bytes[body..];
        // `(content_end, closer_len)`: `-->` is 3 bytes, `--!>` is 4.
        let end = match (find_bytes(tail, b"-->"), find_bytes(tail, b"--!>")) {
            (Some(a), Some(b)) => {
                if a <= b {
                    Some((a, 3))
                } else {
                    Some((b, 4))
                }
            }
            (Some(a), None) => Some((a, 3)),
            (None, Some(b)) => Some((b, 4)),
            (None, None) => None,
        };
        if let Some((end, closer)) = end {
            self.out.push_str("<!--");
            self.emit_opaque(self.slice(body, body + end));
            self.out.push_str("-->");
            self.pos = body + end + closer;
            return;
        }
        let rest = &self.rest()[4..];
        if rest.starts_with(b">") {
            self.out.push_str("<!-- -->");
            self.pos += 5;
        } else if rest.starts_with(b"->") {
            self.out.push_str("<!-- -->");
            self.pos += 6;
        } else {
            // Unterminated at EOF: `goahead` end-handling strips one
            // trailing `--!`, else `--`, else `-` before
            // `handle_comment` (probed: `<!--a--` → `<!--a-->`,
            // `<!--a---` → `<!--a--->`, `<!---` → `<!-- -->`).
            let mut end = len;
            for suffix in [&b"--!"[..], &b"--"[..], &b"-"[..]] {
                if end - body >= suffix.len() && self.bytes[end - suffix.len()..end] == *suffix {
                    end -= suffix.len();
                    break;
                }
            }
            self.out.push_str("<!--");
            self.emit_opaque(self.slice(body, end));
            self.out.push_str("-->");
            self.pos = len;
        }
    }

    /// `<![CDATA[...]]>`: verbatim through `]]>` (auto-closed at EOF),
    /// same ws/empty rules as comments (probed).
    fn parse_cdata(&mut self) {
        let len = self.bytes.len();
        let body = self.pos + 9;
        self.out.push_str("<![CDATA[");
        if let Some(end) = find_bytes(&self.bytes[body..], b"]]>") {
            self.emit_opaque(self.slice(body, body + end));
            self.pos = body + end + 3;
        } else {
            self.emit_opaque(self.slice(body, len));
            self.pos = len;
        }
        self.out.push_str("]]>");
    }

    /// `<?...?>`: ends at the first `>` (auto-closed at EOF), content
    /// verbatim, never ws-collapsed, empty → single space (probed).
    fn parse_pi(&mut self) {
        let len = self.bytes.len();
        let body = self.pos + 2;
        self.out.push_str("<?");
        match self.bytes[body..].iter().position(|&b| b == b'>') {
            Some(end) => {
                let data = self.slice(body, body + end);
                self.out.push_str(if data.is_empty() { " " } else { data });
                self.pos = body + end + 1;
            }
            None => {
                let data = self.slice(body, len);
                self.out.push_str(if data.is_empty() { " " } else { data });
                self.pos = len;
            }
        }
        self.out.push('>');
    }

    /// `<!DOCTYPE...>` (and case variants): keyword uppercased, the
    /// first 8 data chars (`doctype` + one) stripped, `>\n` appended
    /// (probed). Completes at EOF too.
    fn parse_doctype(&mut self) {
        let len = self.bytes.len();
        let body = self.pos + 2;
        let end = self.bytes[body..]
            .iter()
            .position(|&b| b == b'>')
            .map(|e| body + e)
            .unwrap_or(len);
        let data = self.slice(body, end);
        // Python strips 8 *characters* (`doctype` + one); cut at the
        // 8th char boundary (byte 8 can split a multibyte char).
        let rest = data
            .char_indices()
            .nth(8)
            .map(|(i, _)| &data[i..])
            .unwrap_or("");
        self.out.push_str("<!DOCTYPE ");
        self.out.push_str(rest);
        if rest.is_empty() {
            self.out.push(' ');
        }
        self.out.push_str(">\n");
        self.pos = (end + 1).min(len);
    }

    /// Bogus comment (`<!...>` that is not a comment/CDATA/doctype, or
    /// `</` + non-letter): content to the first `>` or EOF (probed).
    fn parse_bogus(&mut self, body: usize) {
        let len = self.bytes.len();
        let end = self.bytes[body..]
            .iter()
            .position(|&b| b == b'>')
            .map(|e| body + e)
            .unwrap_or(len);
        self.out.push_str("<!--");
        self.emit_opaque(self.slice(body, end));
        self.out.push_str("-->");
        self.pos = (end + 1).min(len);
    }

    /// Raw-text content for the just-pushed `name` element.
    /// `plaintext` runs to EOF and never closes (probed). script/style
    /// run verbatim to the matching close; xmp/iframe/noembed/noframes
    /// and plaintext escape without decoding. RCDATA (title/textarea)
    /// decodes entities with the text-path decoder while watching for
    /// the close — interleaved, because the `&#` break swallows the
    /// close itself (probed R16).
    fn parse_raw(&mut self, name: &str) {
        if name == "plaintext" {
            let content = self.slice(self.pos, self.bytes.len());
            self.emit_escaped_raw(content);
            self.pos = self.bytes.len();
            self.pop_raw();
            return;
        }
        if !is_rcdata(name) {
            self.parse_rawtext(name);
            return;
        }
        self.parse_rcdata(name);
    }

    /// RAWTEXT content to the matching close (`</name` + ignored
    /// junk + `>`). A junk scan that never reaches `>` drops the
    /// would-be close: content ends before `</`, the element
    /// auto-closes (probed).
    fn parse_rawtext(&mut self, name: &str) {
        let len = self.bytes.len();
        let content_start = self.pos;
        let mut k = content_start;
        let mut found: Option<(usize, usize)> = None;
        let mut dropped_at: Option<usize> = None;
        while k < len {
            if self.bytes[k] == b'<'
                && k + 1 < len
                && self.bytes[k + 1] == b'/'
                && match_close_name(&self.bytes[k + 2..], name)
            {
                match scan_endtag_junk(self.bytes, k + 2 + name.len()) {
                    Some(after) => {
                        found = Some((k, after));
                    }
                    // The failed scan swallows everything after it
                    // (probed), so no later close can rescue it.
                    None => {
                        dropped_at = Some(k);
                    }
                }
                break;
            }
            k += 1;
        }
        match found {
            Some((close_at, after)) => {
                self.emit_rawtext(self.slice(content_start, close_at), is_verbatim_raw(name));
                self.pos = after;
                self.pop_raw();
            }
            None => {
                let end = dropped_at.unwrap_or(len);
                self.emit_rawtext(self.slice(content_start, end), is_verbatim_raw(name));
                self.pos = len;
                self.pop_raw();
            }
        }
    }

    /// Emit RAWTEXT content (collapsed when whitespace-only outside
    /// `<pre>`/`<textarea>` — probed): script/style verbatim, the rest
    /// escaped without decoding.
    fn emit_rawtext(&mut self, content: &str, verbatim: bool) {
        if content.is_empty() {
            // No output for empty RAWTEXT.
        } else if self.pre_depth == 0 && content.bytes().all(is_html_ws) {
            self.out.push_str(collapse_ws(content));
        } else if verbatim {
            self.out.push_str(content);
        } else {
            escape_text(content, &mut self.out);
        }
    }

    /// Emit escaped raw content (plaintext; same collapse rule).
    fn emit_escaped_raw(&mut self, content: &str) {
        self.emit_rawtext(content, false);
    }

    /// RCDATA content (title/textarea): tags-as-text, entities decoded
    /// with the shared text-path decoder (the `&#` break swallows the
    /// close itself — probed R16, hence interleaved rather than
    /// pre-scanned). The pending buffer holds exactly this chunk: the
    /// raw open flushed it before pushing.
    fn parse_rcdata(&mut self, name: &str) {
        let len = self.bytes.len();
        while self.pos < len {
            if self.bytes[self.pos] == b'<'
                && self.pos + 1 < len
                && self.bytes[self.pos + 1] == b'/'
                && match_close_name(&self.bytes[self.pos + 2..], name)
            {
                match scan_endtag_junk(self.bytes, self.pos + 2 + name.len()) {
                    Some(after) => {
                        self.flush_text();
                        self.pos = after;
                        self.pop_raw();
                        return;
                    }
                    // The would-be close drops: content ends before
                    // `</`, the element auto-closes (probed).
                    None => {
                        self.flush_text();
                        self.pos = len;
                        self.pop_raw();
                        return;
                    }
                }
            }
            if self.bytes[self.pos] == b'<' {
                // Tags-as-text (probed).
                self.pending.push('<');
                self.pos += 1;
                continue;
            }
            if self.bytes[self.pos] == b'&' {
                if self.entity_text() == EntityFlow::Break {
                    break;
                }
                continue;
            }
            let c = self.slice(self.pos, len).chars().next().unwrap_or('\0');
            self.pending.push(c);
            self.pos += c.len_utf8().max(1);
        }
        // EOF without a close: auto-close (probed).
        self.flush_text();
        self.pop_raw();
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

/// `</` + `name` + delimiter (whitespace, `/`, `>` — probed) at
/// the start of `s`, case-insensitive. EOF is NOT a delimiter: the
/// `interesting` lookahead needs a char, so `</name` at EOF stays
/// content (probed: `<script>x</script` keeps it verbatim).
fn match_close_name(s: &[u8], name: &str) -> bool {
    let nb = name.as_bytes();
    if s.len() <= nb.len() || !s[..nb.len()].eq_ignore_ascii_case(nb) {
        return false;
    }
    // `plaintext` never matches (it never closes) — handled by caller.
    matches!(
        s[nb.len()],
        b'\t' | b'\n' | 0x0c | b'\r' | b' ' | b'/' | b'>'
    )
}

/// Scan end-tag junk from `k` (`locatetagend` past the tag name):
/// whitespace and `/` separators, then `attrfind_tolerant` items —
/// an attrname, an optional `=value` (quoted values scan unbounded
/// for the matching quote) — repeating to `>`. Returns the byte past
/// `>`, or `None` when the scan stops without one.
///
/// Bare quotes never protect `>` (probed: `</p "a>b">` closes at the
/// inner `>`); only `name="..."` values do (probed: `</p a=">">`
/// closes at the outer). An unterminated quoted value backtracks the
/// post-`=` ws to empty and keeps `=` + empty-bare when ws was
/// consumed (probed: `</p a= "y>` closes at the inner `>`); with no
/// ws the value group matches empty (CPython rewinds past the
/// attrname), so the next attrname needs attrfind's lookbehind — a
/// quote, whitespace or `/` (probed rewind pair: `</p a="y>`
/// swallows, `</p a ="y>` closes at the inner `>`).
fn scan_endtag_junk(bytes: &[u8], mut k: usize) -> Option<usize> {
    let len = bytes.len();
    // Junk always starts at whitespace, `/`, `>` or EOF (where the
    // name scan stopped), so the first iteration finishes or skips a
    // separator; this seed value is never consulted.
    let mut prev = bytes[k.saturating_sub(1)];
    loop {
        while k < len && (is_html_ws(bytes[k]) || bytes[k] == b'/') {
            prev = bytes[k];
            k += 1;
        }
        if k >= len {
            return None;
        }
        if bytes[k] == b'>' {
            return Some(k + 1);
        }
        if !(prev == b'"' || prev == b'\'' || is_html_ws(prev) || prev == b'/') {
            return None;
        }
        // Attrname: `[^ws/>]` + `[^ws/=>]*` (`=`, `<`, quotes and NUL
        // all allowed first — probed).
        prev = bytes[k];
        k += 1;
        while k < len && !is_html_ws(bytes[k]) && !matches!(bytes[k], b'/' | b'=' | b'>') {
            prev = bytes[k];
            k += 1;
        }
        let after_name = k;
        let prev_after_name = prev;
        // The value group (`=value`) is optional as a whole: only
        // advance past the pre-`=` gap when `=` is actually there.
        let mut v = k;
        while v < len && is_html_ws(bytes[v]) {
            v += 1;
        }
        if v >= len || bytes[v] != b'=' {
            continue;
        }
        let eq_pos = v;
        v += 1;
        while v < len && is_html_ws(bytes[v]) {
            v += 1;
        }
        if v < len && (bytes[v] == b'"' || bytes[v] == b'\'') {
            let q = bytes[v];
            let mut w = v + 1;
            while w < len && bytes[w] != q {
                w += 1;
            }
            if w >= len {
                // Unterminated quote: CPython backtracks the
                // post-`=` ws to empty and keeps `=` + empty-bare
                // (`a= "y` → `a=""`), so the scan continues after
                // the ws — but only when ws was actually consumed
                // (probed: `</p a= "y>` closes inner, `</p a="y>`
                // swallows). With no ws the value group matches
                // empty: back after the attrname.
                if v > eq_pos + 1 {
                    k = eq_pos + 1;
                    prev = b'=';
                } else {
                    k = after_name;
                    prev = prev_after_name;
                }
                continue;
            }
            prev = q;
            k = w + 1;
        } else {
            // Bare value (possibly empty): to whitespace, `>` or EOF.
            while v < len && bytes[v] != b'>' && !is_html_ws(bytes[v]) {
                v += 1;
            }
            k = v;
        }
    }
}

/// Find a byte needle in a haystack.
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&i| &haystack[i..i + needle.len()] == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(html: &str) -> String {
        normalize_bs(html).html
    }

    #[test]
    fn golden_bs4_normalize() {
        let raw =
            include_str!("../../../../fixtures/tasks_mail/send/F-SEND.bs4_normalize.golden.json");
        let fixture: serde_json::Value = serde_json::from_str(raw).expect("golden fixture parses");
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
    fn endtag_junk_is_attrfind_shaped_not_quote_aware() {
        // Bare quotes never protect `>`: close at the inner one.
        assert_eq!(norm("<p>x</p \"a>b\">tail"), "<p>x</p>b\"&gt;tail");
        assert_eq!(
            norm("<script>x</script \"a>b\">tail"),
            "<script>x</script>b\"&gt;tail"
        );
        assert_eq!(
            norm("<title>x</title \"a>b\">tail"),
            "<title>x</title>b\"&gt;tail"
        );
        // Only `name="..."` values protect `>`: close at the outer.
        assert_eq!(norm("<p>x</p a=\">\">tail"), "<p>x</p>tail");
        // Unterminated quoted value: rewind, then the lookbehind decides.
        assert_eq!(norm("<p>x</p a=\"y>TAIL"), "<p>x</p>");
        assert_eq!(norm("<p>x</p a =\"y>TAIL"), "<p>x</p>TAIL");
        // `=` and `/` start junk attrs.
        assert_eq!(norm("<p>x</p =>tail"), "<p>x</p>tail");
        assert_eq!(norm("<p>x</p/x>tail"), "<p>x</p>tail");
    }

    #[test]
    fn endtag_junk_backtracks_post_eq_ws_before_unterminated_quote() {
        // CPython backtracks the post-`=` ws to empty and keeps `=` +
        // empty-bare, so the scan continues after the ws and closes at
        // the inner `>`.
        assert_eq!(norm("<p>x</p a= \"y>TAIL"), "<p>x</p>TAIL");
        assert_eq!(norm("<p>x</p a= 'y>TAIL"), "<p>x</p>TAIL");
        assert_eq!(norm("<p>x</p a=  \"y>TAIL"), "<p>x</p>TAIL");
        assert_eq!(norm("<p>x</p a=\t\"y>TAIL"), "<p>x</p>TAIL");
        // Same shape at the raw/RCDATA sites.
        assert_eq!(
            norm("<script>x</script a= \"y>TAIL</script>END"),
            "<script>x</script>TAILEND"
        );
        assert_eq!(
            norm("<title>x</title a= \"y>TAIL</title>END"),
            "<title>x</title>TAILEND"
        );
        // Controls: no post-`=` ws means a full rewind (drops); the
        // pre-ws-only, terminated and EOF shapes are unchanged.
        assert_eq!(norm("<p>x</p a= \"b\">TAIL"), "<p>x</p>TAIL");
        assert_eq!(norm("<p>x</p a= \"y"), "<p>x</p>");
        assert_eq!(norm("<p>x</p a=\x0b\"y>TAIL"), "<p>x</p>TAIL");
    }

    #[test]
    fn open_tag_backtracks_post_eq_ws_before_unterminated_quote() {
        // The open-tag analog: `a` keeps `=` + empty-bare (`a=""`),
        // then `"y` parses as a bare attrname and the tag completes
        // (attrs render sorted).
        assert_eq!(norm("<p a= \"y>z</p>"), "<p \"y=\"\" a=\"\">z</p>");
        assert_eq!(norm("<p a= 'y>z</p>"), "<p 'y=\"\" a=\"\">z</p>");
        assert_eq!(norm("<p a=  \"y>z</p>"), "<p \"y=\"\" a=\"\">z</p>");
        assert_eq!(norm("<p a=\t\"y>z</p>"), "<p \"y=\"\" a=\"\">z</p>");
        // Controls: no-ws and EOF drop the tag; pre-ws-only and
        // terminated shapes are unchanged; `\x0b` is a bare value.
        assert_eq!(norm("<p a=\"y>z</p>"), "");
        assert_eq!(norm("<p a =\"y>z</p>"), "<p =\"y=\"\" a=\"\">z</p>");
        assert_eq!(norm("<p a= \"b\">z</p>"), "<p a=\"b\">z</p>");
        assert_eq!(norm("<p a= \"y"), "");
        assert_eq!(norm("<p a=\x0b\"y>z</p>"), "<p a='\x0b\"y'>z</p>");
        // No post-`=` ws: the value group matches empty, so the scan
        // resumes after the attrname past attrfind's lookbehind — a
        // skipped pre-`=` sep or a quote-final name completes the tag.
        assert_eq!(norm("<p a\"=\"y>z</p>"), "<p =\"y=\"\" a\"=\"\">z</p>");
        assert_eq!(norm("<p a\" =\"y>z</p>"), "<p =\"y=\"\" a\"=\"\">z</p>");
        assert_eq!(norm("<p ab =\"y>z</p>"), "<p =\"y=\"\" ab=\"\">z</p>");
        assert_eq!(norm("<p ab=\"y>z</p>"), "");
        assert_eq!(norm("<p a\t=\"y>z</p>"), "<p =\"y=\"\" a=\"\">z</p>");
        assert_eq!(
            norm("<p b=\"1\" a =\"y>z</p>"),
            "<p =\"y=\"\" a=\"\" b=\"1\">z</p>"
        );
        assert_eq!(norm("<p a =\"y"), "");
    }

    #[test]
    fn raw_close_with_junk_at_eof_drops() {
        assert_eq!(norm("<script>x</script <b"), "<script>x</script>");
        assert_eq!(norm("<style>x</style <b"), "<style>x</style>");
        assert_eq!(norm("<title>x</title <b"), "<title>x</title>");
        assert_eq!(norm("<textarea>x</textarea <b"), "<textarea>x</textarea>");
        assert_eq!(norm("<iframe>x</iframe <b"), "<iframe>x</iframe>");
        // …but `</name` at EOF (no delimiter) stays content.
        assert_eq!(norm("<script>x</script"), "<script>x</script</script>");
        assert_eq!(norm("<title>x</title"), "<title>x&lt;/title</title>");
        // A later complete close still wins mid-input.
        assert_eq!(
            norm("<script>x</script <b>y</script>TAIL"),
            "<script>x</script>yTAIL"
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
    fn nul_is_a_tag_name_char() {
        assert_eq!(norm("<p\x00>x</p>"), "<p\x00>x</p\x00>");
        assert_eq!(norm("<p>x</p\x00>tail"), "<p>xtail</p>");
        assert_eq!(norm("<p\x00>x</p\x00>tail"), "<p\x00>x</p\x00>tail");
    }

    #[test]
    fn dash_bang_gt_closes_comments() {
        assert_eq!(norm("<!--x--!>"), "<!--x-->");
        assert_eq!(norm("<!--a--!>b-->"), "<!--a-->b--&gt;");
        assert_eq!(norm("<!--a--!>TAIL"), "<!--a-->TAIL");
        assert_eq!(norm("<!--x-->TAIL"), "<!--x-->TAIL");
    }

    #[test]
    fn unterminated_comment_at_eof_strips_one_suffix() {
        // `goahead` end-handling strips one trailing `--!`, else `--`,
        // else `-` before `handle_comment`.
        assert_eq!(norm("<!--a--"), "<!--a-->");
        assert_eq!(norm("<!--a-"), "<!--a-->");
        assert_eq!(norm("<!--a--!"), "<!--a-->");
        assert_eq!(norm("<!--a---"), "<!--a--->");
        assert_eq!(norm("<!-----"), "<!----->");
        assert_eq!(norm("<!---"), "<!-- -->");
        // Controls: no strip without the suffix at EOF.
        assert_eq!(norm("<!--a--x"), "<!--a--x-->");
        assert_eq!(norm("<!--a-- "), "<!--a-- -->");
        assert_eq!(norm("<!--a-- x"), "<!--a-- x-->");
        assert_eq!(norm("<!--!"), "<!--!-->");
        assert_eq!(norm("<!--a"), "<!--a-->");
        assert_eq!(norm("<!--"), "<!-- -->");
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
        let n = normalize_bs("<title><mention-component entity_identifier=\"u1\"/></title>");
        assert!(n.mentions.is_empty());
        let n =
            normalize_bs("<p title=\"<mention-component entity_identifier=&#34;u1&#34;>\">x</p>");
        assert!(n.mentions.is_empty());
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
    fn unescape_attr_backtracks_and_maps_invalids() {
        assert_eq!(unescape_attr("&copyx;"), "©x;");
        assert_eq!(unescape_attr("&notit;"), "¬it;");
        assert_eq!(unescape_attr("&unknown;"), "&unknown;");
        assert_eq!(unescape_attr("&#65"), "A");
        assert_eq!(unescape_attr("&#0;"), "\u{fffd}");
        assert_eq!(unescape_attr("&#1;"), "");
        assert_eq!(unescape_attr("&#128;"), "€");
        assert_eq!(unescape_attr("&#129;"), "\u{81}");
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
