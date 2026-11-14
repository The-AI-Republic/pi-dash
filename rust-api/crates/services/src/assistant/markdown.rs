//! Assistant markdown → sanitized HTML (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/utils/markdown_converter.py:444-469` (full CommonMark + GFM),
//! replacing the previous naive paragraph-per-blank-line port of
//! `assistant/runtime/markdown.py:1-38`. The naive port wrapped markdown
//! verbatim inside `<p>` tags (`"<p>## Summary</p>"`), which the frontend's
//! `RichTextView` then rendered literally — raw `##`, `**`, `` ` `` and
//! list markers appeared instead of formatted headings, lists, code blocks,
//! tables, links and emphasis.
//!
//! The fix reuses the same rendering rules as
//! `operations/issues.rs::markdown_to_html_port`: pulldown-cmark with
//! `ENABLE_TABLES | ENABLE_STRIKETHROUGH | ENABLE_TASKLISTS`, raw HTML
//! disabled (`html=False`), Tiptap-shaped HTML emission, and the shared
//! `space::sanitize` allowlist (ammonia/`nh3` parity). `description_json`
//! stays empty downstream (no Tiptap converter), as before.

/// Fallback HTML (`markdown_converter.py:22,25,38`): empty input and sanitizer
/// failures both render an empty paragraph.
pub const EMPTY_HTML: &str = "<p></p>";
/// `validate_html_content` size cap (`content_validator.py:219-221`): inputs
/// over 10 MiB fail sanitization.
pub const MAX_HTML_BYTES: usize = 10 * 1024 * 1024;

/// markdown-it's `escapeHtml`: exactly `&<>"` (`markdown-it-py`
/// `common/utils.py`). Notably NOT `'`.
fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// `_is_safe_url` (`markdown_converter.py:208-214`): relative URLs and the
/// sanitizer's schemes only (`SAFE_PROTOCOLS =
/// {"http","https","mailto","tel"}`, `content_validator.py:159`).
fn is_safe_url(url: &str) -> bool {
    // Strip ASCII \t\r\n everywhere, then leading C0 controls and spaces.
    let scrubbed: String = url
        .chars()
        .filter(|ch| !matches!(ch, '\t' | '\r' | '\n'))
        .collect();
    let scrubbed = scrubbed.trim_start_matches(|ch: char| ch.is_ascii_control() || ch == ' ');
    let mut chars = scrubbed.chars();
    let Some(first) = chars.next() else {
        return true;
    };
    if !first.is_ascii_alphabetic() {
        return true;
    }
    let mut end = 1;
    for ch in chars {
        if ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.') {
            end += ch.len_utf8();
        } else if ch == ':' {
            let scheme = scrubbed[..end].to_ascii_lowercase();
            if !matches!(scheme.as_str(), "http" | "https" | "mailto" | "tel") {
                return false;
            }
            return ipv6_literally_valid(&scrubbed[end + 1..]);
        } else {
            return true;
        }
    }
    true
}

fn ipv6_literally_valid(rest: &str) -> bool {
    let authority = rest
        .strip_prefix("//")
        .map(|after| after.split(['/', '?', '#']).next().unwrap_or(after));
    let Some(authority) = authority else {
        return true;
    };
    let host = authority.rsplit('@').next().unwrap_or(authority);
    if !host.starts_with('[') {
        return true;
    }
    let Some(close) = host.find(']') else {
        return false;
    };
    let inner = host[1..close].split('%').next().unwrap_or("");
    let tail = &host[close + 1..];
    if !(tail.is_empty() || tail.starts_with(':')) {
        return false;
    }
    inner.parse::<std::net::Ipv6Addr>().is_ok()
}

/// One resolved markdown event: pulldown's stream after the task-list
/// pass (markers stripped or restored as literal text).
#[derive(Debug, Clone)]
enum MdEvent {
    StartHeading(u32),
    EndHeading,
    StartParagraph,
    EndParagraph,
    StartQuote,
    EndQuote,
    CodeBlock(Option<String>, String),
    StartList(Option<u64>, bool),
    EndList(bool),
    StartItem(bool, bool),
    EndItem(bool),
    StartTable,
    EndTable,
    StartHead,
    EndHead,
    StartRow,
    EndRow,
    StartCell(bool),
    EndCell(bool),
    StartEm,
    EndEm,
    StartStrong,
    EndStrong,
    StartStrike,
    EndStrike,
    StartLink(String, String),
    EndLink,
    Image(String, String, String),
    Text(String),
    Code(String),
    HtmlInline(String),
    HtmlBlock(String),
    SoftBreak,
    HardBreak,
    Rule,
}

/// Lower pulldown events to [`MdEvent`], deciding task lists
/// (`_resolve_task_lists`, `markdown_converter.py:232-272`): a bullet list
/// becomes a `taskList` only when EVERY item carries a marker; elsewhere
/// the marker is restored as literal `[x]`/`[ ]` text.
fn resolve_task_events(events: &[pulldown_cmark::Event]) -> Vec<MdEvent> {
    use pulldown_cmark::{Event, Tag, TagEnd};
    #[derive(Debug)]
    struct ListFrame {
        ordered_start: Option<u64>,
        item_has_marker: Vec<bool>,
        item_indices: Vec<usize>,
    }
    let mut list_stack: Vec<ListFrame> = Vec::new();
    let mut list_decision: Vec<Option<bool>> = vec![None; events.len()];
    let mut item_task: Vec<Option<bool>> = vec![None; events.len()];
    for (index, event) in events.iter().enumerate() {
        match event {
            Event::Start(Tag::List(start)) => list_stack.push(ListFrame {
                ordered_start: *start,
                item_has_marker: Vec::new(),
                item_indices: Vec::new(),
            }),
            Event::End(TagEnd::List(_)) => {
                if let Some(frame) = list_stack.pop() {
                    let as_task = frame.ordered_start.is_none()
                        && !frame.item_indices.is_empty()
                        && frame.item_has_marker.iter().all(|has| *has);
                    let mut depth = 0;
                    for back in (0..index).rev() {
                        match &events[back] {
                            Event::End(TagEnd::List(_)) => depth += 1,
                            Event::Start(Tag::List(_)) => {
                                if depth == 0 {
                                    list_decision[back] = Some(as_task);
                                    break;
                                }
                                depth -= 1;
                            }
                            _ => {}
                        }
                    }
                    list_decision[index] = Some(as_task);
                    for (item_idx, has) in
                        frame.item_indices.iter().zip(frame.item_has_marker.iter())
                    {
                        if *has {
                            item_task[*item_idx] = Some(as_task);
                        }
                    }
                }
            }
            Event::Start(Tag::Item) => {
                if let Some(frame) = list_stack.last_mut() {
                    frame.item_indices.push(index);
                    let has = matches!(events.get(index + 1), Some(Event::TaskListMarker(_)));
                    frame.item_has_marker.push(has);
                }
            }
            _ => {}
        }
    }
    let mut out: Vec<MdEvent> = Vec::with_capacity(events.len());
    let mut index = 0;
    let mut item_stack: Vec<(bool, bool, usize, bool)> = Vec::new();
    let mut lstrip_next_text = false;
    let mut image_stack: Vec<(String, String, String)> = Vec::new();
    let mut cell_stack: Vec<bool> = Vec::new();
    let mut para_depth: u32 = 0;
    let mut cell_depth: u32 = 0;
    while index < events.len() {
        if let Event::TaskListMarker(checked) = events[index] {
            if para_depth == 0 && cell_depth == 0 {
                if let Some(frame) = item_stack.last_mut() {
                    if !frame.3 {
                        out.push(MdEvent::StartParagraph);
                        frame.3 = true;
                    }
                }
            }
            let is_task = item_stack
                .last()
                .map(|(task, _, _, _)| *task)
                .unwrap_or(false);
            if !is_task {
                out.push(MdEvent::Text(if checked {
                    "[x]".to_owned()
                } else {
                    "[ ]".to_owned()
                }));
            } else if let Some(top) = item_stack.last_mut() {
                top.1 = checked;
            }
            lstrip_next_text = is_task;
            index += 1;
            continue;
        }
        let is_item_end = matches!(&events[index], Event::End(TagEnd::Item));
        let is_block_start = matches!(
            &events[index],
            Event::Start(
                Tag::Paragraph
                    | Tag::Heading { .. }
                    | Tag::BlockQuote(_)
                    | Tag::CodeBlock(_)
                    | Tag::HtmlBlock
                    | Tag::List(_)
                    | Tag::FootnoteDefinition(_)
                    | Tag::Table(_)
            ) | Event::Rule
                | Event::Html(_)
        );
        let is_inline_content = matches!(
            &events[index],
            Event::Text(_)
                | Event::Code(_)
                | Event::SoftBreak
                | Event::HardBreak
                | Event::InlineHtml(_)
                | Event::FootnoteReference(_)
                | Event::InlineMath(_)
                | Event::DisplayMath(_)
                | Event::Start(
                    Tag::Emphasis
                        | Tag::Strong
                        | Tag::Strikethrough
                        | Tag::Link { .. }
                        | Tag::Image { .. }
                )
                | Event::End(
                    TagEnd::Emphasis
                        | TagEnd::Strong
                        | TagEnd::Strikethrough
                        | TagEnd::Link
                        | TagEnd::Image
                )
        );
        if is_item_end {
            if let Some(frame) = item_stack.last_mut() {
                if frame.3 {
                    out.push(MdEvent::EndParagraph);
                    frame.3 = false;
                }
            }
        } else if is_block_start {
            if para_depth == 0 {
                if let Some(frame) = item_stack.last_mut() {
                    if frame.3 {
                        out.push(MdEvent::EndParagraph);
                        frame.3 = false;
                    }
                }
            }
        } else if is_inline_content && para_depth == 0 && cell_depth == 0 {
            if let Some(frame) = item_stack.last_mut() {
                if !frame.3 {
                    out.push(MdEvent::StartParagraph);
                    frame.3 = true;
                }
            }
        }
        match &events[index] {
            Event::Start(Tag::Paragraph) => {
                para_depth += 1;
                out.push(MdEvent::StartParagraph);
            }
            Event::End(TagEnd::Paragraph) => {
                para_depth = para_depth.saturating_sub(1);
                out.push(MdEvent::EndParagraph);
            }
            Event::Start(Tag::Heading { level, .. }) => {
                out.push(MdEvent::StartHeading(*level as u32))
            }
            Event::End(TagEnd::Heading(_)) => out.push(MdEvent::EndHeading),
            Event::Start(Tag::BlockQuote(_)) => out.push(MdEvent::StartQuote),
            Event::End(TagEnd::BlockQuote(_)) => out.push(MdEvent::EndQuote),
            Event::Start(Tag::CodeBlock(kind)) => {
                use pulldown_cmark::CodeBlockKind;
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                let mut body = String::new();
                let mut cursor = index + 1;
                while cursor < events.len() {
                    match &events[cursor] {
                        Event::Text(text) | Event::Code(text) => body.push_str(text),
                        Event::End(TagEnd::CodeBlock) => break,
                        _ => {}
                    }
                    cursor += 1;
                }
                if let Some(stripped) = body.strip_suffix('\n') {
                    body = stripped.to_owned();
                }
                let fenced = matches!(kind, CodeBlockKind::Fenced(_));
                out.push(MdEvent::CodeBlock(
                    if fenced && !language.is_empty() {
                        Some(language)
                    } else if fenced {
                        Some(String::new())
                    } else {
                        None
                    },
                    body,
                ));
                index = cursor;
            }
            Event::End(TagEnd::CodeBlock) => {}
            Event::Start(Tag::List(start)) => {
                let as_task = list_decision[index].unwrap_or(false);
                out.push(MdEvent::StartList(*start, as_task));
            }
            Event::End(TagEnd::List(_)) => {
                let as_task = list_decision[index].unwrap_or(false);
                out.push(MdEvent::EndList(as_task));
            }
            Event::Start(Tag::Item) => {
                let as_task = item_task[index].unwrap_or(false);
                let open = out.len();
                item_stack.push((as_task, false, open, false));
                out.push(MdEvent::StartItem(as_task, false));
            }
            Event::End(TagEnd::Item) => {
                let (as_task, checked, open, _) =
                    item_stack
                        .pop()
                        .unwrap_or((false, false, usize::MAX, false));
                if let Some(MdEvent::StartItem(_, slot)) = out.get_mut(open) {
                    *slot = checked;
                }
                out.push(MdEvent::EndItem(as_task));
            }
            Event::Start(Tag::Table(_)) => out.push(MdEvent::StartTable),
            Event::End(TagEnd::Table) => out.push(MdEvent::EndTable),
            Event::Start(Tag::TableHead) => out.push(MdEvent::StartHead),
            Event::End(TagEnd::TableHead) => out.push(MdEvent::EndHead),
            Event::Start(Tag::TableRow) => out.push(MdEvent::StartRow),
            Event::End(TagEnd::TableRow) => out.push(MdEvent::EndRow),
            Event::Start(Tag::TableCell) => {
                cell_depth += 1;
                let mut in_head = false;
                let mut depth = 0;
                for back in (0..index).rev() {
                    match &events[back] {
                        Event::Start(Tag::TableHead) if depth == 0 => {
                            in_head = true;
                            break;
                        }
                        Event::End(TagEnd::TableHead) => depth += 1,
                        Event::Start(Tag::TableHead) => {
                            depth -= 1;
                        }
                        Event::Start(Tag::Table(_)) => break,
                        _ => {}
                    }
                }
                cell_stack.push(in_head);
                out.push(MdEvent::StartCell(in_head));
            }
            Event::End(TagEnd::TableCell) => {
                cell_depth = cell_depth.saturating_sub(1);
                let head = cell_stack.pop().unwrap_or(false);
                out.push(MdEvent::EndCell(head));
            }
            Event::Start(Tag::Emphasis) => out.push(MdEvent::StartEm),
            Event::End(TagEnd::Emphasis) => out.push(MdEvent::EndEm),
            Event::Start(Tag::Strong) => out.push(MdEvent::StartStrong),
            Event::End(TagEnd::Strong) => out.push(MdEvent::EndStrong),
            Event::Start(Tag::Strikethrough) => out.push(MdEvent::StartStrike),
            Event::End(TagEnd::Strikethrough) => out.push(MdEvent::EndStrike),
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) => {
                out.push(MdEvent::StartLink(dest_url.to_string(), title.to_string()));
            }
            Event::End(TagEnd::Link) => out.push(MdEvent::EndLink),
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                image_stack.push((dest_url.to_string(), title.to_string(), String::new()));
            }
            Event::End(TagEnd::Image) => {
                let (dest, title, alt) = image_stack.pop().unwrap_or_default();
                if image_stack.is_empty() {
                    out.push(MdEvent::Image(dest, title, alt));
                } else if let Some(parent) = image_stack.last_mut() {
                    parent.2.push_str(&alt);
                }
            }
            Event::Text(text) => {
                let mut content = text.to_string();
                if lstrip_next_text {
                    let stripped = content.trim_start_matches([' ', '\t']).to_owned();
                    content = stripped;
                    lstrip_next_text = false;
                }
                if let Some(image) = image_stack.last_mut() {
                    image.2.push_str(&content);
                } else {
                    out.push(MdEvent::Text(content));
                }
            }
            Event::Code(content) => {
                if let Some(image) = image_stack.last_mut() {
                    image.2.push_str(content);
                } else {
                    out.push(MdEvent::Code(content.to_string()));
                }
            }
            Event::Html(content) => out.push(MdEvent::HtmlBlock(content.to_string())),
            Event::InlineHtml(content) if image_stack.is_empty() => {
                out.push(MdEvent::HtmlInline(content.to_string()));
            }
            Event::InlineHtml(_) => {}
            Event::SoftBreak => {
                if let Some(image) = image_stack.last_mut() {
                    image.2.push(' ');
                } else {
                    out.push(MdEvent::SoftBreak);
                }
            }
            Event::HardBreak => out.push(MdEvent::HardBreak),
            Event::Rule => out.push(MdEvent::Rule),
            Event::FootnoteReference(_) => {}
            Event::TaskListMarker(_) => {}
            _ => {}
        }
        index += 1;
    }
    out
}

fn render_tiptap(events: &[MdEvent], html: &mut String) {
    let mut index = 0;
    while index < events.len() {
        match &events[index] {
            MdEvent::StartHeading(level) => html.push_str(&format!("<h{level}>")),
            MdEvent::EndHeading => {
                let mut level = 1;
                for back in events[..index].iter().rev() {
                    match back {
                        MdEvent::StartHeading(open) => {
                            level = *open;
                            break;
                        }
                        MdEvent::EndHeading => break,
                        _ => {}
                    }
                }
                html.push_str(&format!("</h{level}>"));
            }
            MdEvent::StartParagraph => {
                if !is_lone_image(events, index) {
                    html.push_str("<p>");
                }
            }
            MdEvent::EndParagraph => {
                if !is_lone_image_close(events, index) {
                    html.push_str("</p>");
                }
            }
            MdEvent::StartQuote => html.push_str("<blockquote>"),
            MdEvent::EndQuote => html.push_str("</blockquote>"),
            MdEvent::CodeBlock(language, body) => {
                let body = escape_html(body);
                match language {
                    Some(language) if !language.is_empty() => html.push_str(&format!(
                        "<pre><code class=\"language-{}\">{body}</code></pre>",
                        escape_html(language)
                    )),
                    _ => html.push_str(&format!("<pre><code>{body}</code></pre>")),
                }
            }
            MdEvent::StartList(start, as_task) => match start {
                None if *as_task => html.push_str("<ul data-type=\"taskList\">"),
                None => html.push_str("<ul>"),
                Some(1) => html.push_str("<ol>"),
                Some(number) => html.push_str(&format!("<ol start=\"{number}\">")),
            },
            MdEvent::EndList(_as_task) => {
                let mut ordered = false;
                let mut depth = 0;
                for back in events[..index].iter().rev() {
                    match back {
                        MdEvent::EndList(_) => depth += 1,
                        MdEvent::StartList(start, _) => {
                            if depth == 0 {
                                ordered = start.is_some();
                                break;
                            }
                            depth -= 1;
                        }
                        _ => {}
                    }
                }
                html.push_str(if ordered { "</ol>" } else { "</ul>" });
            }
            MdEvent::StartItem(as_task, checked) => {
                if !as_task {
                    html.push_str("<li>");
                } else {
                    let checkbox = if *checked {
                        "<input type=\"checkbox\" checked=\"checked\">"
                    } else {
                        "<input type=\"checkbox\">"
                    };
                    html.push_str(&format!(
                        "<li data-type=\"taskItem\" data-checked=\"{}\"><label>{checkbox}<span></span></label><div>",
                        if *checked { "true" } else { "false" }
                    ));
                }
            }
            MdEvent::EndItem(as_task) => {
                html.push_str(if *as_task { "</div></li>" } else { "</li>" });
            }
            MdEvent::StartTable => html.push_str("<table><tbody>"),
            MdEvent::EndTable => html.push_str("</tbody></table>"),
            MdEvent::StartHead | MdEvent::EndHead => {}
            MdEvent::StartRow => html.push_str("<tr>"),
            MdEvent::EndRow => html.push_str("</tr>"),
            MdEvent::StartCell(head) => {
                html.push_str(if *head { "<th><p>" } else { "<td><p>" });
            }
            MdEvent::EndCell(head) => {
                html.push_str(if *head { "</p></th>" } else { "</p></td>" });
            }
            MdEvent::StartEm => html.push_str("<em>"),
            MdEvent::EndEm => html.push_str("</em>"),
            MdEvent::StartStrong => html.push_str("<strong>"),
            MdEvent::EndStrong => html.push_str("</strong>"),
            MdEvent::StartStrike => html.push_str("<s>"),
            MdEvent::EndStrike => html.push_str("</s>"),
            MdEvent::StartLink(dest, title) => {
                let mut attrs = String::new();
                if is_safe_url(dest) {
                    attrs.push_str(&format!(" href=\"{}\"", escape_html(dest)));
                }
                if !title.is_empty() {
                    attrs.push_str(&format!(" title=\"{}\"", escape_html(title)));
                }
                html.push_str(&format!("<a{attrs}>"));
            }
            MdEvent::EndLink => html.push_str("</a>"),
            MdEvent::Image(dest, title, alt) => {
                if !is_safe_url(dest) || dest.is_empty() {
                    html.push_str(&escape_html(alt));
                } else {
                    let mut attrs = format!(" src=\"{}\"", escape_html(dest));
                    if !alt.is_empty() {
                        attrs.push_str(&format!(" alt=\"{}\"", escape_html(alt)));
                    }
                    if !title.is_empty() {
                        attrs.push_str(&format!(" title=\"{}\"", escape_html(title)));
                    }
                    html.push_str(&format!("<img{attrs}>"));
                }
            }
            MdEvent::Text(text) => html.push_str(&escape_html(text)),
            MdEvent::Code(code) => html.push_str(&format!("<code>{}</code>", escape_html(code))),
            MdEvent::HtmlInline(content) => html.push_str(&escape_html(content)),
            MdEvent::HtmlBlock(content) => {
                html.push_str(&format!("<p>{}</p>", escape_html(content.trim())));
            }
            MdEvent::SoftBreak => html.push(' '),
            MdEvent::HardBreak => html.push_str("<br>"),
            MdEvent::Rule => html.push_str("<div data-type=\"horizontalRule\"><div></div></div>"),
        }
        index += 1;
    }
}

fn is_lone_image(events: &[MdEvent], open: usize) -> bool {
    if paragraph_depth(events, open) != 0 {
        return false;
    }
    if !matches!(events.get(open + 1), Some(MdEvent::Image(_, _, _))) {
        return false;
    }
    if !matches!(events.get(open + 2), Some(MdEvent::EndParagraph)) {
        return false;
    }
    if let Some(MdEvent::Image(dest, _, _)) = events.get(open + 1) {
        return !dest.is_empty() && is_safe_url(dest);
    }
    false
}

fn is_lone_image_close(events: &[MdEvent], close: usize) -> bool {
    if close < 2 {
        return false;
    }
    if !matches!(events.get(close - 1), Some(MdEvent::Image(_, _, _))) {
        return false;
    }
    if let Some(MdEvent::StartParagraph) = events.get(close - 2) {
        return is_lone_image(events, close - 2);
    }
    false
}

fn paragraph_depth(events: &[MdEvent], open: usize) -> usize {
    let mut depth: usize = 0;
    for event in &events[..open] {
        match event {
            MdEvent::StartQuote | MdEvent::StartItem(_, _) | MdEvent::StartCell(_) => depth += 1,
            MdEvent::EndQuote | MdEvent::EndItem(_) | MdEvent::EndCell(_) => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    depth
}

/// Render paragraphs from markdown (`markdown_to_html` full port):
/// pulldown-cmark CommonMark + GFM tables/strikethrough/tasklists, raw HTML
/// disabled, Tiptap HTML emission. Returns `EMPTY_HTML` for empty input.
pub fn markdown_to_html(body: Option<&str>) -> String {
    let body = body.unwrap_or("");
    if body.trim().is_empty() {
        return EMPTY_HTML.to_owned();
    }
    use pulldown_cmark::{Options, Parser};
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let events: Vec<pulldown_cmark::Event> = Parser::new_ext(body, options).collect();
    let resolved = resolve_task_events(&events);
    let mut html = String::new();
    render_tiptap(&resolved, &mut html);
    if html.is_empty() {
        return EMPTY_HTML.to_owned();
    }
    // Size cap is enforced by to_safe_html via sanitizer; keep render uncapped here.
    html
}

/// Sanitized HTML for an assistant body: render, then run the shared
/// `space::sanitize` allowlist (`validate_html_content` parity). Falls back
/// to `EMPTY_HTML` on sanitizer rejection or empty output (10 MiB cap).
pub fn to_safe_html(body: Option<&str>) -> String {
    let html = markdown_to_html(body);
    if html.len() > MAX_HTML_BYTES {
        return EMPTY_HTML.to_owned();
    }
    match crate::space::sanitize::sanitize_html(&html) {
        crate::space::sanitize::Sanitize::Clean(clean) => {
            if clean.is_empty() {
                EMPTY_HTML.to_owned()
            } else {
                clean
            }
        }
        crate::space::sanitize::Sanitize::Invalid => EMPTY_HTML.to_owned(),
    }
}

/// Compat shim for the previous `sanitize_html` that handled only `<p>/<br>`.
/// Now routes through the space allowlist for full HTML; kept for API stability.
pub fn sanitize_html(html: &str) -> String {
    match crate::space::sanitize::sanitize_html(html) {
        crate::space::sanitize::Sanitize::Clean(clean) => clean,
        crate::space::sanitize::Sanitize::Invalid => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Full markdown expectations: headings, lists, code, tables, links, emphasis,
    // sanitization, and fallback behavior. These vectors replace the previous
    // naive paragraph-wrapping expectations which were the bug.
    const FULL_VECTORS: &[(&str, &str)] = &[
        // headings
        ("## Summary\n\nThis is heading", "<h2>Summary</h2><p>This is heading</p>"),
        ("# Title", "<h1>Title</h1>"),
        // lists
        (
            "- item one\n- item two",
            "<ul><li><p>item one</p></li><li><p>item two</p></li></ul>",
        ),
        (
            "1. first\n2. second",
            "<ol><li><p>first</p></li><li><p>second</p></li></ol>",
        ),
        // code blocks and inline code
        (
            "```python\nprint('hi')\n```",
            "<pre><code class=\"language-python\">print('hi')</code></pre>",
        ),
        ("Inline `code` here", "<p>Inline <code>code</code> here</p>"),
        // tables
        (
            "| A | B |\n|---|---|\n| 1 | 2 |",
            "<table><tbody><tr><th><p>A</p></th><th><p>B</p></th></tr><tr><td><p>1</p></td><td><p>2</p></td></tr></tbody></table>",
        ),
        // bold/italic/links
        (
            "This is **bold** and *italic* and [link](https://example.com)",
            "<p>This is <strong>bold</strong> and <em>italic</em> and <a href=\"https://example.com\" rel=\"noopener noreferrer\">link</a></p>",
        ),
        (
            "## Title\n\n- bullet\n- **bold**\n\n```js\ncode\n```\n\n| col |\n| --- |\n| val |",
            "<h2>Title</h2><ul><li><p>bullet</p></li><li><p><strong>bold</strong></p></li></ul><pre><code class=\"language-js\">code</code></pre><table><tbody><tr><th><p>col</p></th></tr><tr><td><p>val</p></td></tr></tbody></table>",
        ),
        // plain paragraphs still work
        ("hello world", "<p>hello world</p>"),
        ("a\nb", "<p>a b</p>"),
        // blockquote
        ("> quoted", "<blockquote><p>quoted</p></blockquote>"),
    ];

    #[test]
    fn full_markdown_vectors() {
        for (md, expected) in FULL_VECTORS {
            assert_eq!(
                &to_safe_html(Some(md)),
                expected,
                "markdown {md:?} should render {expected}"
            );
        }
    }

    #[test]
    fn empty_and_blank_bodies_fall_back() {
        assert_eq!(markdown_to_html(None), "<p></p>");
        assert_eq!(markdown_to_html(Some("")), "<p></p>");
        assert_eq!(markdown_to_html(Some("  \n\n  ")), "<p></p>");
        assert_eq!(to_safe_html(None), "<p></p>");
        assert_eq!(to_safe_html(Some("")), "<p></p>");
        assert_eq!(to_safe_html(Some("   ")), "<p></p>");
    }

    #[test]
    fn sanitization_preserved() {
        // Raw HTML inside markdown is escaped (html=False), script tags become literal text
        let html = to_safe_html(Some("<script>alert(1)</script>"));
        assert!(
            !html.contains("<script>"),
            "script should be escaped, got {html}"
        );
        assert!(
            html.contains("&lt;script&gt;"),
            "escaped script expected, got {html}"
        );

        // Unsafe javascript: links drop href
        let html = to_safe_html(Some("[click](javascript:alert(1))"));
        assert!(
            !html.contains("javascript:"),
            "javascript: href should be stripped, got {html}"
        );
        assert!(html.contains("click"), "link text kept, got {html}");

        // Html inline escaped
        let html = to_safe_html(Some("Hello <b>bold</b>"));
        // <b> is sanitized but in markdown source <b> is treated as HTML and escaped because html=False,
        // so it should appear escaped
        assert!(
            html.contains("&lt;b&gt;"),
            "inline html escaped, got {html}"
        );
    }

    #[test]
    fn oversized_input_falls_back() {
        let big = "x".repeat(MAX_HTML_BYTES + 1);
        assert_eq!(to_safe_html(Some(&big)), "<p></p>");
    }

    #[test]
    fn no_raw_markdown_in_output() {
        // Ensure no raw markdown syntax remains in rendered output for representative cases
        let cases = [
            "## Summary\n\nHello **world**",
            "- item one\n- item two",
            "```python\nprint('hi')\n```",
            "| A | B |\n|---|---|\n| 1 | 2 |",
            "This is **bold** and *italic*",
        ];
        for md in cases {
            let html = to_safe_html(Some(md));
            // Heuristics: rendered HTML should contain tag brackets, not raw leaders
            assert!(!html.contains("## "), "raw heading marker in {html}");
            assert!(!html.contains("**"), "raw bold marker in {html}");
            assert!(!html.contains("```"), "raw fence in {html}");
            assert!(html.contains("<"), "should contain html tags");
        }
    }
}
