//! `strip_tags` from `pi_dash/utils/html_processor.py` (MLStripper).
//!
//! Ports `MLStripper` (`html_processor.py:10-22`) and `strip_tags`
//! (`:25-28`): a stock `html.parser.HTMLParser` with
//! `convert_charrefs=True` whose only collector is `handle_data`, fed once
//! and never closed. `Page.save()` (`db/models/page.py:70-77`) and
//! `PageVersion.save()` (`:175-182`) both funnel through
//! [`sync_description_stripped`].
//!
//! This is a transliteration of CPython 3.12's `goahead(end=0)`
//! (`html/parser.py`), `parse_starttag`, `check_for_whole_start_tag`,
//! `parse_endtag`, `parse_html_declaration`, `parse_bogus_comment`,
//! `parse_pi`, `parse_comment` and `html.unescape` (`html/__init__.py`)
//! with every handler a no-op except `handle_data`, which appends. The
//! named-entity table and `unescape` live in the shared
//! `pidash_types::html_entities` (generated from `html.entities.html5`).
//!
//! Three behaviors a simpler "drop `<...>` spans" port gets wrong, all
//! verified against the live `strip_tags` (Python 3.12) before committing:
//!
//! * Text chunks are passed through `unescape`: `&amp;` decodes to `&`,
//!   `&nbsp;` to U+00A0, `&#128;` to the euro sign. (This is a different
//!   function from Django's `django.utils.html.strip_tags`, ported at
//!   `api::space::sanitize::strip_tags`, which keeps references verbatim.
//!   The `db` crate cannot reuse that helper anyway: the crate graph runs
//!   `types` → `db` → `services` → `api`.)
//! * A trailing `&...` tail with no `;`/ASCII-space ahead is dropped whole
//!   (`R&D` → `""`, `fish &amp` → `""`; `goahead`'s near-end guard). `feed`
//!   is never followed by `close`, so every other incomplete trailing
//!   construct (open tag, comment, declaration) is dropped the same way.
//! * `<script>`/`<style>`/`<xmp>`/`<iframe>`/`<noembed>`/`<noframes>` switch
//!   the parser to RAWTEXT mode and `<textarea>`/`<title>` to RCDATA mode:
//!   content up to the matching end tag passes through (verbatim, or
//!   entity-decoded for RCDATA), and anything else is dropped.
//!
//! `<![...]>` declarations never raise: 3.12 consumes them to the first
//! `>` (`unknown_decl`/`handle_comment`, both no-ops in `MLStripper`), so
//! stripping is infallible and `save()` always writes normally.

use pidash_types::html_entities::html_unescape;

/// CDATA/RCDATA element (`HTMLParser.CDATA_CONTENT_ELEMENTS` /
/// `RCDATA_CONTENT_ELEMENTS`, `parser.py`). `set_cdata_mode` lowercases the
/// tag before comparing, so matching here is on the lowercased name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CdataElem {
    Script,
    Style,
    Xmp,
    Iframe,
    Noembed,
    Noframes,
    Textarea,
    Title,
    Plaintext,
}

impl CdataElem {
    fn name(self) -> &'static str {
        match self {
            CdataElem::Script => "script",
            CdataElem::Style => "style",
            CdataElem::Xmp => "xmp",
            CdataElem::Iframe => "iframe",
            CdataElem::Noembed => "noembed",
            CdataElem::Noframes => "noframes",
            CdataElem::Textarea => "textarea",
            CdataElem::Title => "title",
            CdataElem::Plaintext => "plaintext",
        }
    }

    /// RCDATA (`textarea`, `title`) content is entity-decoded (`_escapable`
    /// with `convert_charrefs`); RAWTEXT passes through verbatim.
    fn escapable(self) -> bool {
        matches!(self, CdataElem::Textarea | CdataElem::Title)
    }

    /// `set_cdata_mode` (`parser.py`): RAWTEXT elements, RCDATA elements,
    /// and `plaintext`. (`noscript` needs `scripting=True`, which
    /// `MLStripper` never sets, so it parses normally.)
    fn from_tag(tag: &str) -> Option<CdataElem> {
        match tag {
            "script" => Some(CdataElem::Script),
            "style" => Some(CdataElem::Style),
            "xmp" => Some(CdataElem::Xmp),
            "iframe" => Some(CdataElem::Iframe),
            "noembed" => Some(CdataElem::Noembed),
            "noframes" => Some(CdataElem::Noframes),
            "textarea" => Some(CdataElem::Textarea),
            "title" => Some(CdataElem::Title),
            "plaintext" => Some(CdataElem::Plaintext),
            _ => None,
        }
    }
}

/// HTML whitespace for tag/declaration structure (`[\t\n\r\f ]` in
/// `tagfind_tolerant`/`attrfind_tolerant`/`locatetagend` and the `&`
/// near-end guard, `parser.py`). ASCII-only: `\x0b`, `\x85` and non-ASCII
/// spaces are ordinary name/value chars here.
fn is_html_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\x0c' | ' ')
}

/// Python `str.strip()` (used only for the start-tag end check in
/// `parse_starttag`): full Unicode whitespace.
fn is_py_space(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{1c}'..='\u{1f}' | '\u{85}'
    ) || c.is_whitespace()
}

/// Tag-name continuation (`tagfind_tolerant`): everything except HTML
/// whitespace, `/` and `>`.
fn is_tag_name_char(c: char) -> bool {
    !(is_html_space(c) || c == '/' || c == '>')
}

/// Attribute-name first char (`attrfind_tolerant`): `[^\t\n\r\f />]`.
fn is_attr_first_char(c: char) -> bool {
    !(is_html_space(c) || c == '/' || c == '>')
}

/// Attribute-name continuation: `[^\t\n\r\f /=>]*`.
fn is_attr_char(c: char) -> bool {
    !(is_html_space(c) || c == '/' || c == '=' || c == '>')
}

/// `MLStripper.feed(html)` without `close()`: collect `handle_data`,
/// drop every tag/comment/declaration/PI and every incomplete trailing
/// construct (`goahead(0)`).
pub fn ml_strip_tags(html: &str) -> String {
    let raw: Vec<char> = html.chars().collect();
    let n = raw.len();
    let mut out = String::new();
    let mut i = 0;
    let mut cdata: Option<CdataElem> = None;
    while i < n {
        if let Some(elem) = cdata {
            // `plaintext` never terminates (`interesting = re.compile(r'\Z')`):
            // the rest is data, verbatim.
            if elem == CdataElem::Plaintext {
                out.extend(raw[i..].iter());
                break;
            }
            match find_cdata_end(&raw, i, elem.name()) {
                // `interesting` miss with `cdata_elem` set: `break`
                // (`goahead`); the tail stays buffered, and with no
                // `close()` it is never emitted.
                None => break,
                Some(start) => {
                    // RCDATA decodes entities (`_escapable` with
                    // `convert_charrefs`); RAWTEXT passes through verbatim.
                    if elem.escapable() {
                        out.push_str(&html_unescape(&raw[i..start].iter().collect::<String>()));
                    } else {
                        out.extend(raw[i..start].iter());
                    }
                    // `parse_endtag` at the match: the quote-aware
                    // `locatetagend` scan finds the `>` (skipping quoted
                    // `>`s). With none the parse is incomplete (`-1` →
                    // `break`) and the tail is dropped — the content above
                    // is already emitted, exactly as `goahead` emits
                    // before `parse_endtag` runs.
                    match locatetagend_end(&raw, start + 2) {
                        Some(end) => {
                            i = end;
                            cdata = None;
                        }
                        None => break,
                    }
                    continue;
                }
            }
        }
        // `j = rawdata.find('<', i)`.
        let mut j = i;
        while j < n && raw[j] != '<' {
            j += 1;
        }
        if j == n {
            // No `<` ahead: the `&` near-end guard. A `&` in the last 34
            // chars with no `[\t\n\r\f ;]` after it may be a cut-in-half
            // charref, so the parser waits for more text — which never
            // comes, so the tail is dropped.
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
                    if *c == ';' || is_html_space(*c) {
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
            out.push_str(&html_unescape(&raw[i..j].iter().collect::<String>()));
        }
        i = j;
        if i == n {
            break;
        }
        // `rawdata[i] == '<'` dispatch (`goahead`).
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
                Err(()) => break,
            }
        } else if i + 1 < n {
            // A `<` that opens nothing is literal data.
            out.push('<');
            i += 1;
        } else {
            break;
        }
    }
    out
}

fn starts_with(raw: &[char], at: usize, pat: &str) -> bool {
    let pat: Vec<char> = pat.chars().collect();
    raw.len() >= at + pat.len() && raw[at..at + pat.len()] == pat[..]
}

/// First `>` at or after `from` (`endendtag` / `piclose`, `parser.py:28,55`).
fn find_gt(raw: &[char], from: usize) -> Option<usize> {
    (from..raw.len()).find(|&k| raw[k] == '>')
}

/// CDATA end (`set_cdata_mode`: `</elem(?=[\t\n\r\f />])` with
/// `re.IGNORECASE|re.ASCII`): ASCII-only case-insensitive, no whitespace
/// allowed after `</`, single-char lookahead. Returns the match start.
/// (`re.ASCII` means non-ASCII letters never fold: `</scrıpt>` does not
/// end a `script` section.)
fn find_cdata_end(raw: &[char], from: usize, name: &str) -> Option<usize> {
    let name: Vec<char> = name.chars().collect();
    let mut s = from;
    while s < raw.len() {
        if raw[s] != '<' {
            s += 1;
            continue;
        }
        if s + 1 >= raw.len() || raw[s + 1] != '/' {
            s += 1;
            continue;
        }
        // Fewer chars left than `</` + name: no later `s` can match.
        if s + 2 + name.len() > raw.len() {
            break;
        }
        let mut ok = true;
        for (k, nc) in name.iter().enumerate() {
            if !raw[s + 2 + k].eq_ignore_ascii_case(nc) {
                ok = false;
                break;
            }
        }
        if !ok {
            s += 1;
            continue;
        }
        // Lookahead: exactly one char in `[\t\n\r\f />]` (at end of input
        // there is no match).
        let after = s + 2 + name.len();
        if after < raw.len()
            && (is_html_space(raw[after]) || raw[after] == '/' || raw[after] == '>')
        {
            return Some(s);
        }
        s += 1;
    }
    None
}

/// `locatetagend` extent (`parser.py`): tag name, `[\t\n\r\f /]*`, zero or
/// more `attrfind_tolerant` blocks, optional `>`. The caller guarantees an
/// ASCII letter at `pos` (`<` + letter for start tags, `</` + letter for
/// end tags, `</` + element name for CDATA ends). Returns the index past
/// the `>`, or `None` when no `>` terminates the tag (Python's `-1`).
/// Quoted attribute values are skipped whole, so a `>` inside quotes does
/// not end the tag.
fn locatetagend_end(raw: &[char], pos: usize) -> Option<usize> {
    let mut p = pos;
    while p < raw.len() && is_tag_name_char(raw[p]) {
        p += 1;
    }
    loop {
        while p < raw.len() && (is_html_space(raw[p]) || raw[p] == '/') {
            p += 1;
        }
        match match_attr(raw, p) {
            Some(end) => p = end,
            None => break,
        }
    }
    while p < raw.len() && is_html_space(raw[p]) {
        p += 1;
    }
    // `>?`: the match always succeeds; only a `>` terminates.
    if p < raw.len() && raw[p] == '>' {
        Some(p + 1)
    } else {
        None
    }
}

/// `tagfind_tolerant` at `pos`: the caller guarantees an ASCII letter at
/// `pos`. Returns the lowercased name and the match end (name plus the
/// trailing `(?:[\t\n\r\f ]|/(?!>))*` run).
fn match_tagfind(raw: &[char], pos: usize) -> (String, usize) {
    let mut e = pos + 1;
    while e < raw.len() && is_tag_name_char(raw[e]) {
        e += 1;
    }
    let name: String = raw[pos..e].iter().collect();
    let mut m = e;
    while m < raw.len()
        && (is_html_space(raw[m]) || (raw[m] == '/' && !(m + 1 < raw.len() && raw[m + 1] == '>')))
    {
        m += 1;
    }
    (name.to_lowercase(), m)
}

/// `attrfind_tolerant` at `k`: attribute name with the
/// `(?<=['"\t\n\r\f /])` lookbehind, the backtracking optional value
/// group, and the trailing run. Returns the match end. Values are parsed
/// only for their extent — every handler that would read them is a no-op.
fn match_attr(raw: &[char], k: usize) -> Option<usize> {
    if k == 0 {
        return None;
    }
    let prev = raw[k - 1];
    if !(prev == '\'' || prev == '"' || prev == '/' || is_html_space(prev)) {
        return None;
    }
    if k >= raw.len() || !is_attr_first_char(raw[k]) {
        return None;
    }
    let mut p = k + 1;
    while p < raw.len() && is_attr_char(raw[p]) {
        p += 1;
    }
    // Optional value group, with backtracking (`([\t\n\r\f ]*=[\t\n\r\f
    // ]*(...))?`). The bare alternative matches empty, so on a failing
    // quoted value the post-`=` whitespace retracts and the group still
    // consumes the `=` — unless the quote directly follows the `=`, when
    // the whole group matches empty.
    let save = p;
    let mut q = p;
    while q < raw.len() && is_html_space(raw[q]) {
        q += 1;
    }
    if q < raw.len() && raw[q] == '=' {
        let after_eq = q + 1;
        let mut w = after_eq;
        while w < raw.len() && is_html_space(raw[w]) {
            w += 1;
        }
        if w < raw.len() && (raw[w] == '\'' || raw[w] == '"') {
            let quote = raw[w];
            match (w + 1..raw.len()).find(|&t| raw[t] == quote) {
                Some(t) => q = t + 1,
                // Unterminated quote: retract the whitespace (the bare
                // alternative matches empty there) unless there is none
                // to retract, when the whole value group matches empty.
                None => {
                    q = if w > after_eq { after_eq } else { save };
                }
            }
        } else {
            // Bare value (`(?!['"])[^>\t\n\r\f ]*`, possibly empty).
            q = w;
            while q < raw.len() && raw[q] != '>' && !is_html_space(raw[q]) {
                q += 1;
            }
        }
        p = q;
    }
    while p < raw.len()
        && (is_html_space(raw[p]) || (raw[p] == '/' && !(p + 1 < raw.len() && raw[p + 1] == '>')))
    {
        p += 1;
    }
    Some(p)
}

/// `check_for_whole_start_tag`: `locatetagend` from `i + 1` (the caller
/// guarantees `<[a-zA-Z]`); `Ok(endpos)` or `Err(())` for the `-1`
/// (incomplete, buffer-boundary) cases.
fn check_for_whole_start_tag(raw: &[char], i: usize) -> Result<usize, ()> {
    locatetagend_end(raw, i + 1).ok_or(())
}

/// `parse_starttag`. `Ok((end, cdata))` consumes a tag (setting CDATA
/// mode for the RAWTEXT/RCDATA/`plaintext` elements); `Ok` via the `Raw`
/// path emits `raw[i..end]` verbatim — note: no `unescape`.
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
    Ok(StartTag::Tag {
        end: endpos,
        elem: CdataElem::from_tag(&tag),
    })
}

/// `parse_endtag`. Every end tag is consumed; `handle_endtag` is a no-op.
/// On the real-tag path Python also clears CDATA mode unconditionally —
/// in this port the CDATA branch consumes end tags itself, so the mode is
/// already `None` here and the clear is a no-op, exactly as in Python.
fn parse_endtag(raw: &[char], i: usize) -> Result<usize, ()> {
    // Fast check: no `>` anywhere ahead means incomplete.
    find_gt(raw, i + 2).ok_or(())?;
    // `endtagopen` (`</` + ASCII letter).
    if !(i + 2 < raw.len() && raw[i + 2].is_ascii_alphabetic()) {
        // `</>` is ignored; anything else is a bogus comment (the fast
        // check guarantees `find('>', i+2)` hits).
        if starts_with(raw, i, "</>") {
            return Ok(i + 3);
        }
        return parse_bogus_comment(raw, i);
    }
    // Quote-aware `locatetagend` scan (handles `>` inside quoted attr
    // values); incomplete when no `>` terminates the tag.
    locatetagend_end(raw, i + 2).ok_or(())
}

/// `parse_comment`: `commentclose` (`--!?>`) searched from `i + 4`, else
/// `commentabruptclose` (`-?>`) matched at exactly `i + 4`, else
/// incomplete. `handle_comment` is a no-op, so only the end matters.
fn parse_comment(raw: &[char], i: usize) -> Result<usize, ()> {
    let n = raw.len();
    // `commentclose.search(rawdata, i+4)`.
    let mut s = i + 4;
    while s + 1 < n {
        if raw[s] == '-' && s + 1 < n && raw[s + 1] == '-' {
            if s + 2 < n && raw[s + 2] == '>' {
                return Ok(s + 3);
            }
            if s + 3 < n && raw[s + 2] == '!' && raw[s + 3] == '>' {
                return Ok(s + 4);
            }
        }
        s += 1;
    }
    // `commentabruptclose.match(rawdata, i+4)`.
    if i + 4 < n && raw[i + 4] == '>' {
        return Ok(i + 5);
    }
    if i + 5 < n && raw[i + 4] == '-' && raw[i + 5] == '>' {
        return Ok(i + 6);
    }
    Err(())
}

/// `parse_bogus_comment`: first `>` from `i + 2`. `handle_comment` is a
/// no-op.
fn parse_bogus_comment(raw: &[char], i: usize) -> Result<usize, ()> {
    find_gt(raw, i + 2).map(|gt| gt + 1).ok_or(())
}

/// `parse_pi` (`piclose` is `>`): first `>` from `i + 2`. `handle_pi` is a
/// no-op.
fn parse_pi(raw: &[char], i: usize) -> Result<usize, ()> {
    find_gt(raw, i + 2).map(|gt| gt + 1).ok_or(())
}

/// `parse_html_declaration`. The `<!--` arm is kept for fidelity although
/// the main loop never routes a comment here. Nothing here can fail the
/// way the old `parse_marked_section` did: `<![CDATA[` needs its `]]>`,
/// `<!doctype` (any case) and `<![` need their `>`, and anything else is a
/// bogus comment — every handler on these paths is a no-op in
/// `MLStripper`, so `Err` only means incomplete (drop the tail).
fn parse_html_declaration(raw: &[char], i: usize) -> Result<usize, ()> {
    if starts_with(raw, i, "<!--") {
        return parse_comment(raw, i);
    }
    // Exact case, and only with `_support_cdata` — which `MLStripper`
    // never disables.
    if starts_with(raw, i, "<![CDATA[") {
        return find_cdata_literal(raw, i + 9).ok_or(());
    }
    if raw.len() >= i + 9 && raw[i..i + 9].iter().collect::<String>().to_lowercase() == "<!doctype"
    {
        // `find('>', i+9)`; `handle_decl` is a no-op.
        return find_gt(raw, i + 9).map(|gt| gt + 1).ok_or(());
    }
    if starts_with(raw, i, "<![") {
        // `unknown_decl` when the char before `>` is `]`, else
        // `handle_comment` — both no-ops.
        return find_gt(raw, i + 3).map(|gt| gt + 1).ok_or(());
    }
    parse_bogus_comment(raw, i)
}

/// First `]]>` at or after `from` (`rawdata.find(']]>', i+9)`),
/// returned past the bracket run.
fn find_cdata_literal(raw: &[char], from: usize) -> Option<usize> {
    let mut s = from;
    while s + 2 < raw.len() {
        if raw[s] == ']' && raw[s + 1] == ']' && raw[s + 2] == '>' {
            return Some(s + 3);
        }
        s += 1;
    }
    None
}

/// `Page.save()` (`page.py:70-77`) and `PageVersion.save()` (`:175-182`)
/// share one rule: `description_stripped` is `None` when
/// `description_html` is `""` or `None`, else `strip_tags` of the HTML.
/// Infallible: 3.12's `parse_html_declaration` never raises (unknown
/// `<![...]>` sections are consumed to the first `>`).
pub fn sync_description_stripped(description_html: Option<&str>) -> Option<String> {
    match description_html {
        None => None,
        Some("") => None,
        Some(html) => Some(ml_strip_tags(html)),
    }
}

/// Django's `django.utils.html.strip_tags` — what `Description.save()`
/// (`description.py:23-29`) recomputes `description_stripped` with.
/// Entities pass through verbatim (the `handle_entityref` arm), unlike
/// [`ml_strip_tags`] which decodes them: `"a &amp; b"` stays
/// `"a &amp; b"` here but becomes `"a & b"` there. Loop `_strip_once`
/// while both `<` and `>` remain, stopping when an iteration stops
/// removing `<` (verbatim Django algorithm). Canonical home for the
/// port: the `api::space::sanitize` and
/// `services::runner_runs::lifecycle` twins delegate here.
pub fn django_strip_tags(value: &str) -> String {
    let mut current = value.to_owned();
    loop {
        if !(current.contains('<') && current.contains('>')) {
            break;
        }
        let next = django_strip_once(&current);
        if next.matches('<').count() == current.matches('<').count() {
            break;
        }
        current = next;
    }
    current
}

/// One Django `_strip_once` pass: collect text and verbatim entity
/// references, skip tags (quote-aware), comments and declarations.
fn django_strip_once(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte != b'<' {
            // Copy one full char ('<' is ASCII, so byte detection never
            // splits a multi-byte sequence; entities pass through verbatim
            // like HTMLParser's handle_entityref/handle_charref arms).
            let ch = value[index..].chars().next().expect("char boundary");
            out.push(ch);
            index += ch.len_utf8();
            continue;
        }
        // At '<': comments, declarations, and tags are skipped.
        if value[index..].starts_with("<!--") {
            if let Some(end) = value[index..].find("-->") {
                index += end + 3;
            } else {
                // Unterminated comment: HTMLParser bogus-comments to EOF.
                break;
            }
            continue;
        }
        if value[index..].starts_with("<!") || value[index..].starts_with("<?") {
            if let Some(end) = django_find_tag_end(&value[index..]) {
                index += end;
            } else {
                out.push('<');
                index += 1;
            }
            continue;
        }
        // A '<' followed by ASCII alpha or '/' opens a tag; anything else
        // is literal text (HTMLParser parse-starttag rules).
        let rest = &value[index + 1..];
        let opener = rest.chars().next().unwrap_or('\0');
        if opener.is_ascii_alphabetic() || opener == '/' {
            if let Some(end) = django_find_tag_end(&value[index..]) {
                index += end;
                continue;
            }
        }
        out.push('<');
        index += 1;
    }
    out
}

/// Length in bytes of `<...>` starting at `text[0] == '<'`, respecting
/// single/double quotes; `None` when unterminated.
fn django_find_tag_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut quote: Option<u8> = None;
    let mut index = 1;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(q) = quote {
            if byte == q {
                quote = None;
            }
        } else if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
        } else if byte == b'>' {
            return Some(index + 1);
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn django_strip_tags_matches_django_basics() {
        // Vectors carried over from the `api::space::sanitize` twin plus
        // entity cases verified against live `django.utils.html.strip_tags`.
        assert_eq!(django_strip_tags("<p>why</p>"), "why");
        assert_eq!(django_strip_tags("plain"), "plain");
        assert_eq!(django_strip_tags(""), "");
        assert_eq!(django_strip_tags("<p>a</p><p>b</p>"), "ab");
        // Entities pass through verbatim (handle_entityref arm).
        assert_eq!(django_strip_tags("a &amp; b"), "a &amp; b");
        assert_eq!(django_strip_tags("<p>a &amp; b</p>"), "a &amp; b");
        assert_eq!(django_strip_tags("R&D"), "R&D");
        // Bare '<' without '>' is literal; the loop never runs.
        assert_eq!(django_strip_tags("a < b"), "a < b");
        // Bare '<' with '>' elsewhere but no tag: kept literally.
        assert_eq!(django_strip_tags("a < b > c"), "a < b > c");
    }

    #[test]
    fn django_and_ml_strippers_differ_on_entities() {
        // The two save() paths use different strippers on purpose:
        // `IssueComment.save` (html_processor, decoded) vs
        // `Description.save` (django.utils, verbatim).
        assert_eq!(ml_strip_tags("a &amp; b"), "a & b");
        assert_eq!(django_strip_tags("a &amp; b"), "a &amp; b");
    }
}
