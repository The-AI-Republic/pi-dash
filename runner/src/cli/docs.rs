// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash docs …` — the on-demand CLI reference.
//!
//! The prompt carries only a ≤ 6 KB core (NEWPIDASH-320 §6.6); everything
//! else lives in `runner/docs/cli-reference.md`, embedded here with
//! `include_str!` so the reference always ships with the binary it
//! documents. `pidash docs` lists the topics, `pidash docs <topic>`
//! prints one. An unknown topic exits non-zero and lists the valid ones.

use clap::Args;

/// The embedded reference. Static markdown, no template variables — the
/// same text for every run kind (NEWPIDASH-320 §6.6).
pub const REFERENCE: &str = include_str!("../../docs/cli-reference.md");

/// Reference topic names, in the order they appear in [`REFERENCE`].
/// `pidash docs` with no topic prints this list.
pub const TOPICS: &[&str] = &[
    "issue",
    "comment",
    "workpad",
    "page",
    "label",
    "state",
    "scheduler",
    "pods",
    "run",
    "context",
    "recipes",
];

#[derive(Debug, Args)]
pub struct DocsArgs {
    /// Reference topic to print: issue, comment, workpad, page, label,
    /// state, scheduler, pods, run, context, recipes. With no topic,
    /// lists the topics.
    pub topic: Option<String>,
}

/// Print the reference topic, or the topic list. Returns a process exit
/// code like the other CRUD commands (`run_crud`).
pub fn run(args: DocsArgs) -> i32 {
    match args.topic.as_deref() {
        None => {
            println!("{REFERENCE_TOPICS_LIST}");
            0
        }
        Some(topic) => match topic_text(topic) {
            Some(text) => {
                print!("{text}");
                0
            }
            None => {
                eprintln!(
                    "{{\"error\": \"unknown docs topic '{topic}'; valid topics: {valid}}}",
                    valid = TOPICS.join(", ")
                );
                2
            }
        },
    }
}

const REFERENCE_TOPICS_LIST: &str = "pidash docs [<topic>] — print the CLI reference for one topic.\n\nTopics: issue, comment, workpad, page, label, state, scheduler, pods, run, context, recipes.\n\nThe prompt section `Pi Dash CLI` is the core; this reference is the rest.\n";

/// The markdown section for `topic` (its `## <Name>` header through the
/// next `## ` header), or `None` for an unknown topic. Case-insensitive.
/// Line-anchored: a header name that prefixes another header (`Issue`
/// vs `Issues`) still resolves exactly.
pub fn topic_text(topic: &str) -> Option<&'static str> {
    let name = TOPICS
        .iter()
        .find(|name| name.eq_ignore_ascii_case(topic))?;
    let marker = format!("## {}", capitalize(name));
    let mut pos = 0;
    let mut start = None;
    for line in REFERENCE.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        if start.is_none() {
            if body == marker {
                start = Some(pos);
            }
        } else if body.starts_with("## ") {
            return Some(&REFERENCE[start.unwrap_or(0)..pos]);
        }
        pos += line.len();
    }
    start.map(|s| &REFERENCE[s..])
}

fn capitalize(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::{REFERENCE, TOPICS, topic_text};

    /// Every listed topic is non-empty (NEWPIDASH-320 Done when).
    #[test]
    fn every_topic_is_non_empty() {
        for topic in TOPICS {
            let text = topic_text(topic).unwrap_or_else(|| panic!("missing {topic}"));
            assert!(!text.trim().is_empty(), "{topic} is empty");
        }
    }

    #[test]
    fn unknown_topic_is_none() {
        assert!(topic_text("nope").is_none());
        assert!(topic_text("").is_none());
    }

    #[test]
    fn topic_lookup_is_case_insensitive() {
        assert_eq!(topic_text("Workpad"), topic_text("workpad"));
    }

    /// Every `pidash …` command line in the reference parses with the real
    /// clap `Command` (NEWPIDASH-320 Done when). Backticked spans and `sh`
    /// code-block lines are extracted; `[...]` groups contribute their
    /// contents, `<...>` placeholders take fixed dummy values. Skipped:
    /// the bare `pidash` word, `--help` endings (clap answers those before
    /// the command runs), and shell compositions (`|`, `…`).
    #[test]
    fn every_documented_command_parses() {
        use clap::Parser;
        let mut lines: Vec<String> = Vec::new();
        // Backticked `pidash …` spans.
        let mut rest = REFERENCE;
        while let Some(open) = rest.find("`pidash ") {
            let after = &rest[open + 1..];
            let Some(close) = after.find('`') else {
                break;
            };
            lines.push(after[..close].to_string());
            rest = &after[close + 1..];
        }
        // `sh` code-block lines starting with `pidash `.
        let mut in_sh = false;
        for line in REFERENCE.lines() {
            let trimmed = line.trim();
            if trimmed == "```sh" {
                in_sh = true;
                continue;
            }
            if trimmed == "```" {
                in_sh = false;
                continue;
            }
            if in_sh && trimmed.starts_with("pidash ") {
                lines.push(trimmed.to_string());
            }
        }
        assert!(!lines.is_empty(), "no command lines extracted");
        let mut failures = Vec::new();
        for line in &lines {
            if line == "pidash"
                || line.contains('|')
                || line.contains('…')
                || line.ends_with("--help")
            {
                continue;
            }
            let argv = drop_conflicts(expand_placeholders(line));
            if let Err(err) = crate::cli::Cli::try_parse_from(argv.iter().map(String::as_str)) {
                // `--help`/`--version` surface as errors by design; the
                // extractor already skips `--help` lines.
                if !matches!(
                    err.kind(),
                    clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
                ) {
                    failures.push(format!("{line} :: {err}"));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "reference lines that do not parse:\n{}",
            failures.join("\n")
        );
    }

    /// Replace `[optional]` groups with their contents and `<placeholder>`
    /// with a fixed dummy value, then split into argv words.
    fn expand_placeholders(line: &str) -> Vec<String> {
        // `[...]` groups: keep the contents (flags are genuinely optional).
        let mut out = String::with_capacity(line.len());
        let mut depth: u32 = 0;
        for ch in line.chars() {
            match ch {
                '[' => depth += 1,
                ']' => depth = depth.saturating_sub(1),
                _ => out.push(ch),
            }
        }
        out.split_whitespace()
            .map(|word| {
                if word.starts_with('<') && word.ends_with('>') {
                    dummy_for(&word[1..word.len() - 1]).to_string()
                } else {
                    word.to_string()
                }
            })
            .collect()
    }

    /// Drop the second side of mutually-exclusive flag pairs the reference
    /// documents together (`--description` vs `--description-file`, …):
    /// clap forbids passing both, so no agent could run that combination
    /// either. Each flag still parses — just not both at once.
    fn drop_conflicts(argv: Vec<String>) -> Vec<String> {
        // (flag to keep, flag to drop, whether the dropped flag takes a
        // value that must go with it).
        let pairs = [
            ("--description", "--description-file", true),
            ("--body", "--body-file", true),
            ("--extra-context", "--extra-context-file", true),
            ("--parent", "--clear-parent", false),
            ("--add-label", "--label", true),
            ("--add-label", "--clear-labels", false),
            ("--remove-label", "--label", true),
            ("--remove-label", "--clear-labels", false),
            ("--enable", "--disable", false),
        ];
        let mut out = argv;
        for (keep, drop, takes_value) in pairs {
            if out.iter().any(|w| w == keep) && out.iter().any(|w| w == drop) {
                let mut kept = Vec::with_capacity(out.len());
                let mut skip_next = false;
                for word in out {
                    if skip_next {
                        skip_next = false;
                        continue;
                    }
                    if word == drop {
                        skip_next = takes_value;
                        continue;
                    }
                    kept.push(word);
                }
                out = kept;
            }
        }
        out
    }

    /// Fixed dummy value per placeholder kind. Single tokens only, so a
    /// plain whitespace split is argv-exact.
    fn dummy_for(key: &str) -> &str {
        match key.to_lowercase().as_str() {
            "identifier" | "ident" | "issue" | "proj-123" | "proj-123-or-uuid" => "ENG-42",
            "comment-id" | "page-id" | "uuid" | "id" | "scheduler" | "pod" | "runner"
            | "run-id" => "11111111-1111-1111-1111-111111111111",
            "proj" | "project" | "proj-or-uuid" | "slug" | "name-or-uuid" => "ENG",
            "path" | "file" | "body-file" | "description-file" | "prompt-file"
            | "extra-context-file" | "comment-file" | "handoff-file" => "f.md",
            "title" | "name" | "s" | "line" | "note" | "text" | "md" | "description" | "body"
            | "extra-context" | "color" => "x",
            "parent" => "ENG-1",
            "label" => "bug",
            "field" | "fields" => "name",
            "access" => "public",
            "mode" | "outcome-mode" => "create_issue",
            "outcome" | "result" => "done",
            "state-name" | "state" | "to" | "state-name-or-uuid" => "Todo",
            "order-by" | "order" => "name",
            "group" => "open",
            "status" => "open",
            "c" | "cursor" | "limit" | "n" | "per-page" => "10",
            "query" => "q",
            "iso8601" | "since" | "time" | "dtstart" | "tzid" | "rdate" | "exdate"
            | "updated_at" | "updated-at" => "2026-11-20T09:34:00Z",
            "priority" => "high",
            "topic" => "issue",
            "rrule" | "rule" => "FREQ=DAILY",
            "url" | "repo-url" => "https://example.invalid/x.git",
            _ => "x",
        }
    }
}
