//! `strip_tags` from `pi_dash/utils/html_processor.py` (MLStripper).
//!
//! Ports `MLStripper` (`html_processor.py:10-22`) and `strip_tags`
//! (`:25-28`): a stock `html.parser.HTMLParser` with
//! `convert_charrefs=True` whose only collector is `handle_data`, fed once
//! and never closed. `Page.save()` (`db/models/page.py:70-77`) and
//! `PageVersion.save()` (`:175-182`) both funnel through
//! [`sync_description_stripped`].
//!
//! This is a line-for-line transliteration of CPython's `goahead(end=0)`
//! (`html/parser.py:133-250`), `parse_starttag` (`:300-347`),
//! `check_for_whole_start_tag` (`:351-381`), `parse_endtag` (`:385-422`),
//! `parse_html_declaration` (`:255-272`), `parse_bogus_comment`
//! (`:276-285`), `parse_pi` (`:289-297`), `parse_comment`
//! (`_markupbase.py:168-177`), `parse_marked_section` (`:146-164`),
//! `_scan_name` (`:376-392`) and `html.unescape` (`html/__init__.py:91-135`)
//! with every handler a no-op except `handle_data`, which appends. The
//! named-entity table lives in [`super::entities`] (generated from
//! `html.entities.html5`, same source `unescape` reads).
//!
//! Three behaviors a simpler "drop `<...>` spans" port gets wrong, all
//! verified against the live `strip_tags` before committing:
//!
//! * Text chunks are passed through `unescape`: `&amp;` decodes to `&`,
//!   `&nbsp;` to U+00A0, `&#128;` to the euro sign. (This is a different
//!   function from Django's `django.utils.html.strip_tags`, ported at
//!   `api::space::sanitize::strip_tags`, which keeps references verbatim.
//!   The `db` crate cannot reuse that helper anyway: the crate graph runs
//!   `types` → `db` → `services` → `api`.)
//! * A trailing `&...` tail with no `;`/space ahead is dropped whole
//!   (`R&D` → `""`, `fish &amp` → `""`; `parser.py:143-151`). `feed` is
//!   never followed by `close`, so every other incomplete trailing
//!   construct (open tag, comment, declaration) is dropped the same way.
//! * `<script>`/`<style>` switch the parser to CDATA mode: content up to
//!   the matching end tag passes through verbatim (no tag parsing, no
//!   entity decoding), and anything else is dropped.
//!
//! Python raises `NotImplementedError` out of `save()` for `<![` sections
//! with an unknown (or missing) status keyword (`_markupbase.py:146-164`,
//! `:376-392` via `ParserBase.error`); that surfaces here as
//! [`StripError`], which the write path maps to a 500 exactly as Django's
//! 500 on the same input.

use super::entities::{HTML5_ENTITIES, INVALID_CHARREFS, INVALID_CODEPOINTS};

/// Feed of `<![...>` Python cannot classify.
///
/// Mirrors `ParserBase.error`, which raises `NotImplementedError` out of
/// `MLStripper.feed` (and therefore out of `Page.save()` /
/// `PageVersion.save()`). The write path maps this to a 500, matching
/// Django's behavior byte for byte on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripError {
    char_offset: usize,
}

impl StripError {
    /// Char offset of the `<![` that failed classification.
    pub fn offset(self) -> usize {
        self.char_offset
    }
}

impl std::fmt::Display for StripError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown marked section at offset {}", self.char_offset)
    }
}

impl std::error::Error for StripError {}

/// Script/style CDATA element (`HTMLParser.CDATA_CONTENT_ELEMENTS`,
/// `parser.py:84`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CdataElem {
    Script,
    Style,
}

impl CdataElem {
    fn name(self) -> &'static str {
        match self {
            CdataElem::Script => "script",
            CdataElem::Style => "style",
        }
    }
}

/// Python `re` `\s` on `str`: ASCII whitespace plus the explicit extras
/// (`\x0b`, `\x1c`-`\x1f`, `\x85`) plus Unicode `White_Space`
/// (`char::is_whitespace`). Probed against `re.compile(r'\s')` for every
/// code point below U+3000 plus the U+2000 block, U+3000 and U+FEFF.
fn is_py_space(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{1c}'..='\u{1f}' | '\u{85}'
    ) || c.is_whitespace()
}

/// Tag-name continuation (`tagfind_tolerant`, `parser.py:36`):
/// everything except whitespace, `/`, `>` and NUL.
fn is_tag_name_char(c: char) -> bool {
    !matches!(c, '\t' | '\n' | '\r' | '\x0c' | ' ' | '/' | '>' | '\0')
}

/// Entity-name char (`_charref`, `html/__init__.py:130-132`): everything
/// except `\t \n \x0c space < > & # ;`.
fn is_entity_name_char(c: char) -> bool {
    !matches!(c, '\t' | '\n' | '\x0c' | ' ' | '<' | '>' | '&' | '#' | ';')
}

/// End-tag name char (`endtagfind`, `parser.py:58`).
fn is_end_tag_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | ':')
}

/// Attribute-name first char (`attrfind_tolerant`, `parser.py:37-39`):
/// `[^\s/>]`.
fn is_attr_first_char(c: char) -> bool {
    !(is_py_space(c) || c == '/' || c == '>')
}

/// Attribute-name continuation: `[^\s/=>]*`.
fn is_attr_char(c: char) -> bool {
    !(is_py_space(c) || c == '/' || c == '=' || c == '>')
}

/// `MLStripper.feed(html)` without `close()`: collect `handle_data`,
/// drop every tag/comment/declaration/PI and every incomplete trailing
/// construct (`goahead(0)`, `parser.py:133-250`).
pub fn ml_strip_tags(html: &str) -> Result<String, StripError> {
    let raw: Vec<char> = html.chars().collect();
    let n = raw.len();
    let mut out = String::new();
    let mut i = 0;
    let mut cdata: Option<CdataElem> = None;
    while i < n {
        if let Some(elem) = cdata {
            match find_cdata_end(&raw, i, elem) {
                // `interesting` miss with `cdata_elem` set: `break`
                // (`parser.py:157-158`); the tail stays buffered, and with
                // no `close()` it is never emitted.
                None => break,
                Some((start, gt)) => {
                    out.extend(raw[i..start].iter());
                    // `parse_endtag` at the match (`parser.py:385-422`):
                    // only an ASCII `</elem>` (via `endtagfind`) clears
                    // CDATA mode; anything else the case-insensitive scan
                    // found (e.g. `</ſcript>`) is collected as data
                    // (`parser.py:415-418`) and the mode stays on.
                    match match_endtagfind(&raw, start) {
                        Some(name) if name == elem.name() => {
                            i = gt + 1;
                            cdata = None;
                        }
                        _ => {
                            out.extend(raw[start..gt + 1].iter());
                            i = gt + 1;
                        }
                    }
                    continue;
                }
            }
        }
        // `j = rawdata.find('<', i)` (`parser.py:139`).
        let mut j = i;
        while j < n && raw[j] != '<' {
            j += 1;
        }
        if j == n {
            // No `<` ahead: the `&` near-end guard (`parser.py:143-151`).
            // A `&` in the last 34 chars with no whitespace/`;` after it
            // may be a cut-in-half charref, so the parser waits for more
            // text — which never comes, so the tail is dropped.
            let from = i.max(n.saturating_sub(34));
            let mut amp: Option<usize> = None;
            for k in (from..n).rev() {
                if raw[k] == '&' {
                    amp = Some(k);
                    break;
                }
            }
            if let Some(a) = amp {
                let mut terminated = false;
                for c in raw.iter().skip(a) {
                    if *c == ';' || is_py_space(*c) {
                        terminated = true;
                        break;
                    }
                }
                if !terminated {
                    break;
                }
            }
        }
        if i < j {
            out.push_str(&unescape(&raw[i..j]));
        }
        i = j;
        if i == n {
            break;
        }
        // `rawdata[i] == '<'` dispatch (`parser.py:168-183`).
        if i + 1 < n && raw[i + 1].is_ascii_alphabetic() {
            // `starttagopen` (`<[a-zA-Z]`).
            match parse_starttag(&raw, i, &mut out) {
                Ok(StartTag::Tag { end, elem }) => {
                    i = end;
                    cdata = elem;
                }
                Ok(StartTag::Raw { end }) => {
                    i = end;
                }
                Err(()) => break,
            }
        } else if starts_with(&raw, i, "</") {
            match parse_endtag(&raw, i) {
                Ok(end) => i = end,
                Err(()) => break,
            }
        } else if starts_with(&raw, i, "<!--") {
            match parse_comment(&raw, i) {
                Ok(end) => i = end,
                Err(()) => break,
            }
        } else if starts_with(&raw, i, "<?") {
            match parse_pi(&raw, i) {
                Ok(end) => i = end,
                Err(()) => break,
            }
        } else if starts_with(&raw, i, "<!") {
            match parse_html_declaration(&raw, i) {
                Ok(end) => i = end,
                Err(ParseFail::Incomplete) => break,
                Err(ParseFail::Error(e)) => return Err(e),
            }
        } else if i + 1 < n {
            // A `<` that opens nothing is literal data (`parser.py:179-181`).
            out.push('<');
            i += 1;
        } else {
            break;
        }
    }
    Ok(out)
}

/// `parse_*` failure: incomplete (break, tail dropped) or a classification
/// error (Python raises; see [`StripError`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseFail {
    Incomplete,
    Error(StripError),
}

impl From<StripError> for ParseFail {
    fn from(e: StripError) -> Self {
        ParseFail::Error(e)
    }
}

fn starts_with(raw: &[char], at: usize, pat: &str) -> bool {
    let pat: Vec<char> = pat.chars().collect();
    raw.len() >= at + pat.len() && raw[at..at + pat.len()] == pat[..]
}

/// First `>` at or after `from` (`endendtag` / `piclose`, `parser.py:28,55`).
fn find_gt(raw: &[char], from: usize) -> Option<usize> {
    (from..raw.len()).find(|&k| raw[k] == '>')
}

/// Single-char case-insensitive equality for the CDATA-end scan.
///
/// The scan regex is compiled with `re.I` (`parser.py:122-124`), whose one
/// pragmatic quirk is folding four non-ASCII chars onto ASCII letters —
/// U+0130/U+0131 onto `i`, U+017F onto `s`, U+212A onto `k` — probed
/// against `re` itself across the whole BMP (astral spot-checks fold
/// nothing to ASCII). Everything else compares ASCII case-insensitively.
fn fold_eq(c: char, ascii: char) -> bool {
    if c == ascii {
        return true;
    }
    if c.is_ascii() {
        return c.to_ascii_lowercase() == ascii;
    }
    matches!(
        (c, ascii),
        ('\u{130}', 'i') | ('\u{131}', 'i') | ('\u{17f}', 's') | ('\u{212a}', 'k')
    )
}

/// CDATA end (`interesting` with `cdata_elem`, `parser.py:122-124`):
/// `</`, optional whitespace, the element name (case-insensitive per
/// [`fold_eq`]), optional whitespace, `>`. Returns the match start and the
/// `>` index.
fn find_cdata_end(raw: &[char], from: usize, elem: CdataElem) -> Option<(usize, usize)> {
    let name: Vec<char> = elem.name().chars().collect();
    let mut s = from;
    while s < raw.len() {
        while s < raw.len() && raw[s] != '<' {
            s += 1;
        }
        if s + 1 >= raw.len() || raw[s + 1] != '/' {
            s += 1;
            continue;
        }
        let mut p = s + 2;
        while p < raw.len() && is_py_space(raw[p]) {
            p += 1;
        }
        let mut ok = true;
        for nc in &name {
            if p >= raw.len() || !fold_eq(raw[p], *nc) {
                ok = false;
                break;
            }
            p += 1;
        }
        if !ok {
            s += 1;
            continue;
        }
        while p < raw.len() && is_py_space(raw[p]) {
            p += 1;
        }
        if p < raw.len() && raw[p] == '>' {
            return Some((s, p));
        }
        s += 1;
    }
    None
}

/// `tagfind_tolerant` at `pos` (`parser.py:36`): the caller guarantees an
/// ASCII letter at `pos`. Returns the lowercased name and the match end
/// (name plus the trailing `(?:\s|/(?!>))*` run).
fn match_tagfind(raw: &[char], pos: usize) -> (String, usize) {
    let mut e = pos + 1;
    while e < raw.len() && is_tag_name_char(raw[e]) {
        e += 1;
    }
    let name: String = raw[pos..e].iter().collect();
    let mut m = e;
    while m < raw.len()
        && (is_py_space(raw[m]) || (raw[m] == '/' && !(m + 1 < raw.len() && raw[m + 1] == '>')))
    {
        m += 1;
    }
    (name.to_lowercase(), m)
}

/// `attrfind_tolerant` at `k` (`parser.py:37-39`): attribute name with the
/// `(?<=[\'"\s/])` lookbehind, the backtracking optional value group, and
/// the trailing run. Returns the match end. Values are parsed only for
/// their extent — every handler that would read them is a no-op.
fn match_attr(raw: &[char], k: usize) -> Option<usize> {
    if k == 0 {
        return None;
    }
    let prev = raw[k - 1];
    if !(prev == '\'' || prev == '"' || prev == '/' || is_py_space(prev)) {
        return None;
    }
    if k >= raw.len() || !is_attr_first_char(raw[k]) {
        return None;
    }
    let mut p = k + 1;
    while p < raw.len() && is_attr_char(raw[p]) {
        p += 1;
    }
    // Optional value group, with backtracking to empty when the value
    // alternatives fail (`(\s*=+\s*(...))?`).
    let save = p;
    let mut q = p;
    while q < raw.len() && is_py_space(raw[q]) {
        q += 1;
    }
    if q < raw.len() && raw[q] == '=' {
        q += 1;
        while q < raw.len() && raw[q] == '=' {
            q += 1;
        }
        while q < raw.len() && is_py_space(raw[q]) {
            q += 1;
        }
        if q < raw.len() && raw[q] == '\'' {
            match (q + 1..raw.len()).find(|&t| raw[t] == '\'') {
                Some(t) => q = t + 1,
                // `'[^']*'` fails; `"(...)"` and the bare alternative
                // (blocked by the `(?![\'"])` lookahead) fail too, so the
                // whole value group matches empty.
                None => q = save,
            }
        } else if q < raw.len() && raw[q] == '"' {
            match (q + 1..raw.len()).find(|&t| raw[t] == '"') {
                Some(t) => q = t + 1,
                None => q = save,
            }
        } else {
            while q < raw.len() && raw[q] != '>' && !is_py_space(raw[q]) {
                q += 1;
            }
        }
        p = q;
    }
    while p < raw.len()
        && (is_py_space(raw[p]) || (raw[p] == '/' && !(p + 1 < raw.len() && raw[p + 1] == '>')))
    {
        p += 1;
    }
    Some(p)
}

/// `check_for_whole_start_tag` (`parser.py:351-381`): `Ok(endpos)` or
/// `Err(())` for the `-1` (incomplete, buffer-boundary) cases.
fn check_for_whole_start_tag(raw: &[char], i: usize) -> Result<usize, ()> {
    // `locatestarttagend_tolerant` (`parser.py:40-54`): `<`, tag name, then
    // `[\s/]*` + attributes, then trailing whitespace. The match cannot
    // fail — the caller guarantees `<[a-zA-Z]`.
    let mut pos = i + 1;
    while pos < raw.len() && is_tag_name_char(raw[pos]) {
        pos += 1;
    }
    loop {
        while pos < raw.len() && (is_py_space(raw[pos]) || raw[pos] == '/') {
            pos += 1;
        }
        match match_attr(raw, pos) {
            Some(end) => pos = end,
            None => break,
        }
    }
    while pos < raw.len() && is_py_space(raw[pos]) {
        pos += 1;
    }
    let j = pos;
    if j >= raw.len() {
        // End of input.
        return Err(());
    }
    let next = raw[j];
    if next == '>' {
        return Ok(j + 1);
    }
    if next == '/' {
        if j + 1 < raw.len() && raw[j + 1] == '>' {
            return Ok(j + 2);
        }
        // Buffer boundary (`startswith("/", j)` always holds here).
        return Err(());
    }
    if next.is_ascii_alphabetic() || next == '=' || next == '/' {
        // End of input in or before an attribute value.
        return Err(());
    }
    // Bogus input: `j > i` always holds (at least `<x` matched).
    Ok(j)
}

/// `parse_starttag` (`parser.py:300-347`). `Ok((end, cdata))` consumes a
/// tag (setting CDATA mode for script/style); `Ok` via the `Raw` path
/// emits `raw[i..end]` verbatim — note: no `unescape` (`parser.py:338`).
enum StartTag {
    Tag { end: usize, elem: Option<CdataElem> },
    Raw { end: usize },
}

fn parse_starttag(raw: &[char], i: usize, out: &mut String) -> Result<StartTag, ()> {
    let endpos = check_for_whole_start_tag(raw, i)?;
    let (tag, mut k) = match_tagfind(raw, i + 1);
    while k < endpos {
        match match_attr(raw, k) {
            Some(end) => k = end,
            None => break,
        }
    }
    let end_slice: String = raw[k..endpos].iter().collect();
    let end_trimmed: String = end_slice.trim_matches(|c: char| is_py_space(c)).to_string();
    if end_trimmed != ">" && end_trimmed != "/>" {
        out.extend(raw[i..endpos].iter());
        return Ok(StartTag::Raw { end: endpos });
    }
    if end_trimmed == "/>" {
        return Ok(StartTag::Tag {
            end: endpos,
            elem: None,
        });
    }
    let elem = match tag.as_str() {
        "script" => Some(CdataElem::Script),
        "style" => Some(CdataElem::Style),
        _ => None,
    };
    Ok(StartTag::Tag { end: endpos, elem })
}

/// `parse_endtag` (`parser.py:385-422`) with no CDATA element set (the main
/// loop only dispatches here outside CDATA mode). Every end tag is
/// consumed; `handle_endtag` is a no-op.
fn parse_endtag(raw: &[char], i: usize) -> Result<usize, ()> {
    let gt = find_gt(raw, i + 1).ok_or(())?;
    if match_endtagfind(raw, i).is_some() {
        return Ok(gt + 1);
    }
    match match_tagfind_opt(raw, i + 2) {
        None => {
            if starts_with(raw, i, "</>") {
                Ok(i + 3)
            } else {
                // `parse_bogus_comment`: `find('>', i+2)` — `gt` exists.
                Ok(gt + 1)
            }
        }
        Some((_, m)) => {
            let gt2 = find_gt(raw, m).ok_or(())?;
            Ok(gt2 + 1)
        }
    }
}

/// `endtagfind` at `i` (`parser.py:58`): `</`, whitespace, ASCII-letter
/// name, whitespace, `>`. Returns the lowercased name.
fn match_endtagfind(raw: &[char], i: usize) -> Option<String> {
    let mut p = i + 2;
    while p < raw.len() && is_py_space(raw[p]) {
        p += 1;
    }
    if p >= raw.len() || !raw[p].is_ascii_alphabetic() {
        return None;
    }
    let mut e = p + 1;
    while e < raw.len() && is_end_tag_name_char(raw[e]) {
        e += 1;
    }
    let mut m = e;
    while m < raw.len() && is_py_space(raw[m]) {
        m += 1;
    }
    if m < raw.len() && raw[m] == '>' {
        Some(raw[p..e].iter().collect::<String>().to_lowercase())
    } else {
        None
    }
}

/// `tagfind_tolerant` that may fail (end-tag fallback path,
/// `parser.py:398`).
fn match_tagfind_opt(raw: &[char], pos: usize) -> Option<(String, usize)> {
    if pos >= raw.len() || !raw[pos].is_ascii_alphabetic() {
        return None;
    }
    let (name, m) = match_tagfind(raw, pos);
    Some((name, m))
}

/// `parse_comment` (`_markupbase.py:168-177`): `<!--` plus the first
/// `--\s*>`. Unterminated comments are incomplete. `handle_comment` is a
/// no-op, so only the end position matters.
fn parse_comment(raw: &[char], i: usize) -> Result<usize, ()> {
    let mut s = i + 4;
    while s + 1 < raw.len() {
        if raw[s] == '-' && raw[s + 1] == '-' {
            let mut p = s + 2;
            while p < raw.len() && is_py_space(raw[p]) {
                p += 1;
            }
            if p < raw.len() && raw[p] == '>' {
                return Ok(p + 1);
            }
        }
        s += 1;
    }
    Err(())
}

/// `parse_bogus_comment` (`parser.py:276-285`): first `>` from `i+2`.
/// `handle_comment` is a no-op.
fn parse_bogus_comment(raw: &[char], i: usize) -> Result<usize, ()> {
    find_gt(raw, i + 2).map(|gt| gt + 1).ok_or(())
}

/// `parse_pi` (`parser.py:289-297`): first `>` from `i+2` (`piclose`,
/// `parser.py:28`). `handle_pi` is a no-op.
fn parse_pi(raw: &[char], i: usize) -> Result<usize, ()> {
    find_gt(raw, i + 2).map(|gt| gt + 1).ok_or(())
}

/// `parse_html_declaration` (`parser.py:255-272`). The `<!--` arm is kept
/// for fidelity although the main loop never routes a comment here.
fn parse_html_declaration(raw: &[char], i: usize) -> Result<usize, ParseFail> {
    if starts_with(raw, i, "<!--") {
        return parse_comment(raw, i).map_err(|()| ParseFail::Incomplete);
    }
    if starts_with(raw, i, "<![") {
        return parse_marked_section(raw, i);
    }
    if raw.len() >= i + 9 && raw[i..i + 9].iter().collect::<String>().to_lowercase() == "<!doctype"
    {
        // `find('>', i+9)`; `handle_decl` is a no-op.
        return find_gt(raw, i + 9)
            .map(|gt| gt + 1)
            .ok_or(ParseFail::Incomplete);
    }
    parse_bogus_comment(raw, i).map_err(|()| ParseFail::Incomplete)
}

/// `_scan_name` (`_markupbase.py:376-392`): `declstart` is the `<![`
/// position for the error offset. `Ok(None)` means end-of-buffer
/// (incomplete); `Err` mirrors `ParserBase.error` (Python raises).
fn scan_name(
    raw: &[char],
    p: usize,
    declstart: usize,
) -> Result<(Option<String>, usize), StripError> {
    let n = raw.len();
    if p == n {
        return Ok((None, 0));
    }
    if !raw[p].is_ascii_alphabetic() {
        return Err(StripError {
            char_offset: declstart,
        });
    }
    let mut e = p + 1;
    while e < n && (raw[e].is_ascii_alphanumeric() || matches!(raw[e], '-' | '_' | '.')) {
        e += 1;
    }
    while e < n && is_py_space(raw[e]) {
        e += 1;
    }
    if e == n {
        return Ok((None, 0));
    }
    let name: String = raw[p..e]
        .iter()
        .collect::<String>()
        .trim_end_matches(|c: char| is_py_space(c))
        .to_string();
    Ok((Some(name.to_lowercase()), e))
}

/// `parse_marked_section` (`_markupbase.py:146-164`): `<![name[ ... ]]>`
/// (or `]>` for the MS-Office trio). Unknown status keywords raise in
/// Python (`ParserBase.error`); `unknown_decl` is a no-op.
fn parse_marked_section(raw: &[char], i: usize) -> Result<usize, ParseFail> {
    let (name, _) = match scan_name(raw, i + 3, i)? {
        (None, _) => return Err(ParseFail::Incomplete),
        (Some(name), j) => (name, j),
    };
    let ms_close = name == "temp"
        || name == "cdata"
        || name == "ignore"
        || name == "include"
        || name == "rcdata";
    let ms_office = name == "if" || name == "else" || name == "endif";
    if !ms_close && !ms_office {
        return Err(StripError { char_offset: i }.into());
    }
    // `_markedsectionclose` (`]\s*]\s*>`) or `_msmarkedsectionclose`
    // (`]\s*>`), searched from `i+3`.
    let mut s = i + 3;
    while s < raw.len() {
        if raw[s] == ']' {
            let mut p = s + 1;
            while p < raw.len() && is_py_space(raw[p]) {
                p += 1;
            }
            if !ms_close {
                if p < raw.len() && raw[p] == '>' {
                    return Ok(p + 1);
                }
            } else if p < raw.len() && raw[p] == ']' {
                let mut q = p + 1;
                while q < raw.len() && is_py_space(raw[q]) {
                    q += 1;
                }
                if q < raw.len() && raw[q] == '>' {
                    return Ok(q + 1);
                }
            }
        }
        s += 1;
    }
    Err(ParseFail::Incomplete)
}

/// `html.unescape` (`html/__init__.py:122-135`): the `_charref` regex
/// (`:130-132`) with `_replace_charref` (`:91-120`).
fn unescape(raw: &[char]) -> String {
    let n = raw.len();
    let mut out = String::with_capacity(n);
    let mut i = 0;
    while i < n {
        if raw[i] != '&' {
            out.push(raw[i]);
            i += 1;
            continue;
        }
        if let Some((rep, end)) = match_numeric_ref(raw, i) {
            out.push_str(&rep);
            i = end;
            continue;
        }
        if let Some((rep, end)) = match_named_ref(raw, i) {
            out.push_str(&rep);
            i = end;
            continue;
        }
        out.push('&');
        i += 1;
    }
    out
}

/// Numeric arm of `_charref`: `&(#[0-9]+;?|#[xX][0-9a-fA-F]+;?)`.
fn match_numeric_ref(raw: &[char], i: usize) -> Option<(String, usize)> {
    let n = raw.len();
    if i + 2 >= n || raw[i + 1] != '#' {
        return None;
    }
    let (num, mut end) = if raw[i + 2] == 'x' || raw[i + 2] == 'X' {
        let mut d = i + 3;
        while d < n && raw[d].is_ascii_hexdigit() {
            d += 1;
        }
        if d == i + 3 {
            return None;
        }
        (parse_saturating_hex(&raw[i + 3..d]), d)
    } else {
        let mut d = i + 2;
        while d < n && raw[d].is_ascii_digit() {
            d += 1;
        }
        if d == i + 2 {
            return None;
        }
        (parse_saturating_dec(&raw[i + 2..d]), d)
    };
    if end < n && raw[end] == ';' {
        end += 1;
    }
    Some((decode_numeric(num), end))
}

fn parse_saturating_dec(digits: &[char]) -> u64 {
    let mut num: u64 = 0;
    for c in digits {
        let d = (*c as u64).wrapping_sub('0' as u64);
        num = num.saturating_mul(10).saturating_add(d);
    }
    num
}

fn parse_saturating_hex(digits: &[char]) -> u64 {
    let mut num: u64 = 0;
    for c in digits {
        let d = c.to_digit(16).unwrap_or(0) as u64;
        num = num.saturating_mul(16).saturating_add(d);
    }
    num
}

/// `_replace_charref` numeric half (`html/__init__.py:93-110`). Saturation
/// preserves Python's outcomes: any value past `0x10FFFF` (including
/// digit strings that overflow `u64`) decodes to U+FFFD.
fn decode_numeric(num: u64) -> String {
    // The `u32` gate comes first: Python compares the full int, so a value
    // past `u32::MAX` never hits the override table (a truncating cast
    // would alias it onto a small entry).
    if num <= u32::MAX as u64 {
        if let Ok(pos) = INVALID_CHARREFS.binary_search_by(|&(k, _)| k.cmp(&(num as u32))) {
            return INVALID_CHARREFS[pos].1.to_string();
        }
    }
    if (0xD800..=0xDFFF).contains(&num) || num > 0x10FFFF {
        return '\u{FFFD}'.to_string();
    }
    if INVALID_CODEPOINTS.binary_search(&(num as u32)).is_ok() {
        return String::new();
    }
    char::from_u32(num as u32)
        .map(|c| c.to_string())
        .unwrap_or_else(|| '\u{FFFD}'.to_string())
}

/// Named arm of `_charref`: up to 32 `[^\t\n\f <&#;]` chars plus an
/// optional `;`, resolved by `_replace_charref`'s named half
/// (`html/__init__.py:111-120`): exact hit, else longest prefix without a
/// trailing `;`... (implemented as the longest table hit on a proper
/// prefix), else the literal `&` + name.
fn match_named_ref(raw: &[char], i: usize) -> Option<(String, usize)> {
    let n = raw.len();
    let mut p = i + 1;
    while p < n && p - (i + 1) < 32 && is_entity_name_char(raw[p]) {
        p += 1;
    }
    if p == i + 1 {
        return None;
    }
    let mut end = p;
    if end < n && raw[end] == ';' {
        end += 1;
    }
    let key: String = raw[i + 1..end].iter().collect();
    if let Some(rep) = lookup_entity(&key) {
        return Some((rep.to_string(), end));
    }
    // Longest matching proper prefix (`range(len(s)-1, 1, -1)`): `x` is a
    // char count, so rebuild the prefix per length.
    let chars: Vec<char> = key.chars().collect();
    for x in (2..chars.len()).rev() {
        let prefix: String = chars[..x].iter().collect();
        if let Some(rep) = lookup_entity(&prefix) {
            let rest: String = chars[x..].iter().collect();
            return Some((rep.to_string() + &rest, end));
        }
    }
    Some(('&'.to_string() + &key, end))
}

fn lookup_entity(name: &str) -> Option<&'static str> {
    HTML5_ENTITIES
        .binary_search_by(|&(k, _)| k.cmp(name))
        .ok()
        .map(|pos| HTML5_ENTITIES[pos].1)
}

/// `Page.save()` (`page.py:70-77`) and `PageVersion.save()` (`:175-182`)
/// share one rule: `description_stripped` is `None` when
/// `description_html` is `""` or `None`, else `strip_tags` of the HTML.
/// The `Result` carries [`StripError`] exactly where Python's `save()`
/// raises.
pub fn sync_description_stripped(
    description_html: Option<&str>,
) -> Result<Option<String>, StripError> {
    match description_html {
        None => Ok(None),
        Some("") => Ok(None),
        Some(html) => ml_strip_tags(html).map(Some),
    }
}
