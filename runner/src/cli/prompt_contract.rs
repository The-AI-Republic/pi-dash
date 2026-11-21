//! Whole-prompt contract test (NEWPIDASH-325 §6.9): the prompts teach
//! `pidash` commands, and the CLI is the authority on what parses. This
//! test renders nothing itself — it reads the rendered-prompt fixtures the
//! Rust composer test exports
//! (`rust-api/fixtures/prompting/rendered/*.md`) and holds five contracts:
//!
//! 1. Every `pidash …` command line parses with the real clap `Command`
//!    (`Cli::command().try_get_matches_from`), after mechanical
//!    placeholder substitution. A span that names a command without its
//!    required arguments (e.g. the `pidash run finish` mention whose syntax
//!    lives in `lifecycle`) passes only when its subcommand path resolves.
//! 2. `--handoff-file` appears only on coding-task renders (the lifecycle
//!    syntax line owns it there).
//! 3. Every `PIDASH_*` variable a render promises is set by `apply_run_env`
//!    for that kind (observed through `login_shell_command`, i.e. what the
//!    agent process actually receives).
//! 4. The never-run sentence agrees with the generated classification: no
//!    taught command resolves to an operator-classified top-level command.
//! 5. Every quoted section reference (`see "…"`, `as "…"`, `comes from "…"`,
//!    `set by "…"`) matches a heading rendered for that kind.
//!
//! The same parsing runs over every `pidash docs` topic. Placeholder rules
//! mirror the docs legend (`runner/docs/cli-reference.md`): angle brackets
//! mark values you supply, square brackets mark flags you may omit.
//!
//! A prompt change that teaches an unparsable command fails here, not in
//! production. The `missing_fixture_proof` test below pins that once with
//! an inline bad command.

use clap::{CommandFactory, ValueEnum};
use std::collections::BTreeMap;
use std::path::PathBuf;

use super::{Cli, CommandClass, classify_name};
use crate::cli::docs::{TOPICS, topic_text};
use crate::cli::page::PageAccess;
use crate::cli::run_cmd::{FinishResult, Outcome};
use crate::util::containment::BackendKind;
use crate::util::shell::{RUN_ENV, RunEnv};

/// Rendered-prompt fixture directory (written by the Rust composer test
/// `rendered_prompts_match_fixtures`).
fn rendered_dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../rust-api/fixtures/prompting/rendered"
    ))
}

/// `(kind, stem, text)` for every rendered fixture. The kind is the recipe
/// kind the filename starts with.
fn fixture_texts() -> Vec<(&'static str, String, String)> {
    let dir = rendered_dir();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|_| panic!("rendered fixtures missing: {}", dir.display()))
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    entries.sort();
    assert!(
        !entries.is_empty(),
        "no rendered fixtures in {}",
        dir.display()
    );
    entries
        .into_iter()
        .map(|path| {
            let stem = path
                .file_stem()
                .expect("fixture stem")
                .to_string_lossy()
                .into_owned();
            let kind = if stem.starts_with("coding-task") {
                "coding-task"
            } else if stem.starts_with("review") {
                "review"
            } else if stem.starts_with("test") {
                "test"
            } else if stem.starts_with("scheduler") {
                "scheduler"
            } else {
                panic!("fixture {stem} names no known recipe kind");
            };
            let text = std::fs::read_to_string(&path).expect("fixture readable");
            (kind, stem, text)
        })
        .collect()
}

/// Backticked spans of one line (odd segments of a backtick split).
fn backticked_spans(line: &str) -> Vec<&str> {
    line.split('`')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, span)| span)
        .collect()
}

/// Raw `pidash …` candidates from rendered markdown: backticked spans that
/// start with `pidash `, plus indented/fenced code lines that do.
fn extract_candidates(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            if let Some(rest) = trimmed.strip_prefix("pidash ") {
                let _ = rest;
                out.push(trimmed.to_owned());
            }
            continue;
        }
        for span in backticked_spans(line) {
            if span.starts_with("pidash ") {
                out.push(span.to_owned());
            }
        }
        // Indented code blocks (4+ spaces, no backticks, no fence).
        if line.starts_with("    ") && trimmed.starts_with("pidash ") {
            let owned = trimmed.to_owned();
            if !out.contains(&owned) {
                out.push(owned);
            }
        }
    }
    out
}

/// Replace `$(…)` shell substitutions with a dummy value (no nesting in
/// prompts: one `$(git rev-parse …)` shape).
fn strip_command_substitutions(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("$(") {
        out.push_str(&rest[..start]);
        out.push('x');
        let mut depth = 0;
        let mut end = None;
        for (i, ch) in rest[start..].char_indices() {
            if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                depth -= 1;
                if depth == 0 {
                    end = Some(start + i + 1);
                    break;
                }
            }
        }
        rest = match end {
            Some(e) => &rest[e..],
            None => {
                out.push_str(&rest[start + 2..]);
                ""
            }
        };
    }
    out.push_str(rest);
    out
}

/// Dummy value for a `<placeholder>` name. Names the prompt actually uses;
/// anything else becomes `x` (a value-shaped token is all clap checks).
fn dummy_for(name: &str) -> &'static str {
    match name {
        "state" | "state-name" | "state-name-or-UUID" => "In Progress",
        "identifier" | "issue" | "id" | "PROJ-123-or-UUID" => "SAMPLE-1",
        "sibling" => "SAMPLE-9",
        "later-child" => "SAMPLE-3",
        "earlier-child" => "SAMPLE-2",
        "parent" | "parent-identifier" => "SAMPLE-0",
        "project" | "PROJ-or-UUID" | "PROJ-123" => "SAMPLE",
        "path" | "file" | "body-file" | "finding" | "comment-file" => ".pidash/x.md",
        "name" | "title" | "summary" | "note" | "line" | "s" => "x",
        "url" => "https://example.com/pr/7",
        "topic" | "command" => "issue",
        "scheduler" => "nightly-audit",
        "slug" => "x",
        "color" => "#ff0000",
        "priority" => "medium",
        "status" => "open",
        "label" => "bug",
        "order" | "sort" => "rank",
        "N" | "per-page" | "limit" | "cursor" => "10",
        "since" | "time" | "iso8601" | "dtstart" | "tzid" | "rdate" | "exdate" | "updated_at" => {
            "2026-01-01T00:00:00Z"
        }
        "uuid" | "run-id" | "comment-id" | "comment_id" => "00000000-0000-0000-000000000003",
        "branch" | "base" | "head" => "main",
        "mode" | "outcome-mode" => "create_issue",
        "query" | "text" | "prompt" | "description" | "body" | "access" | "rrule"
        | "extra-context" | "pod" | "runner" => "x",
        _ => "x",
    }
}

/// Split a command string on whitespace, respecting single and double
/// quotes (quotes do not nest in prompts).
fn shlex_split(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut in_token = false;
    for ch in s.chars() {
        match (quote, ch) {
            (None, '\'') | (None, '"') => {
                quote = Some(ch);
                in_token = true;
            }
            (Some(q), c) if c == q => quote = None,
            (None, c) if c.is_whitespace() => {
                if in_token {
                    tokens.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            _ => {
                current.push(ch);
                in_token = true;
            }
        }
    }
    if in_token {
        tokens.push(current);
    }
    tokens
}

/// Byte index of the first shell metacharacter (`|` or `>`), skipping
/// `<…>` placeholders, quoted spans and `$(…)` substitutions (a `>` inside
/// any of those is not a redirect). Char-based so multi-byte text cannot
/// split a boundary.
fn shell_cut(s: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (byte, ch) = chars[i];
        match ch {
            '\'' | '"' => {
                i += 1;
                while i < chars.len() && chars[i].1 != ch {
                    i += 1;
                }
                i += 1;
            }
            '$' if chars.get(i + 1).is_some_and(|(_, c)| *c == '(') => {
                let mut depth = 0;
                while i < chars.len() {
                    match chars[i].1 {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }
            '<' => {
                if let Some(end) = s[byte..].find('>') {
                    let target = byte + end + 1;
                    while i < chars.len() && chars[i].0 < target {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            '|' | '>' => return Some(byte),
            _ => i += 1,
        }
    }
    None
}

/// True when `s` contains `&&` or `;` outside `<…>` placeholders, quoted
/// spans and `$(…)` substitutions (a `;` inside `<plain-language TL;DR>`
/// is placeholder text, not a command separator).
fn has_shell_combinators(s: &str) -> bool {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i].1 {
            '\'' | '"' => {
                let quote = chars[i].1;
                i += 1;
                while i < chars.len() && chars[i].1 != quote {
                    i += 1;
                }
                i += 1;
            }
            '$' if chars.get(i + 1).is_some_and(|(_, c)| *c == '(') => {
                let mut depth = 0;
                while i < chars.len() {
                    match chars[i].1 {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }
            '<' => {
                if let Some(end) = s[chars[i].0..].find('>') {
                    let target = chars[i].0 + end + 1;
                    while i < chars.len() && chars[i].0 < target {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            '&' if chars.get(i + 1).is_some_and(|(_, c)| *c == '&') => return true,
            ';' => return true,
            _ => i += 1,
        }
    }
    false
}

/// One expanded variant: the argv plus the sorted indices of the optional
/// `[…]` groups it includes (provenance for the conflict rule below).
type Expanded = (Vec<String>, Vec<usize>);

/// Expand one raw candidate into argv variants: optional `[…]` groups (with
/// and without), `<a|b|c>` alternations, then `<placeholder>` dummies.
fn expand_candidate(raw: &str) -> Vec<Expanded> {
    // A pipeline/redirect teaches the bare command too (`pidash workpad
    // get | jq …`): parse the head before the first shell metacharacter.
    let head = shell_cut(raw).map(|i| raw[..i].trim_end()).unwrap_or(raw);
    assert!(
        !has_shell_combinators(head),
        "candidate combines shell commands, which the contract cannot parse: {raw}"
    );
    let head = strip_command_substitutions(head);
    // Locate the `[…]` optional groups left to right (no nesting in prompts
    // or docs).
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let mut search = 0;
    while let Some(open) = head[search..].find('[') {
        let open = search + open;
        if let Some(rel) = head[open..].find(']') {
            groups.push((open, open + rel));
            search = open + rel + 1;
        } else {
            break;
        }
    }
    // Powerset over group indices.
    let mut argvs: Vec<Expanded> = Vec::new();
    for mask in 0..(1usize << groups.len()) {
        let mut included = Vec::new();
        let mut variant = head.clone();
        for (index, (open, close)) in groups.iter().enumerate().rev() {
            if mask & (1 << index) != 0 {
                let inner = variant[*open + 1..*close].trim().to_owned();
                variant.replace_range(*open..=*close, &inner);
                included.push(index);
            } else {
                variant.replace_range(*open..=*close, " ");
            }
        }
        included.sort_unstable();
        let variant = variant.split_whitespace().collect::<Vec<_>>().join(" ");
        let tokens = shlex_split(&variant);
        // Expand `<a|b|c>` alternations.
        let mut token_options: Vec<Vec<String>> = vec![Vec::new()];
        for token in tokens {
            if token.starts_with('<') && token.ends_with('>') && token.contains('|') {
                let inner = &token[1..token.len() - 1];
                let alts: Vec<String> = inner.split('|').map(|alt| alt.to_owned()).collect();
                token_options = token_options
                    .into_iter()
                    .flat_map(|prefix| {
                        alts.iter().map(move |alt| {
                            let mut next = prefix.clone();
                            next.push(alt.clone());
                            next
                        })
                    })
                    .collect();
            } else if token.starts_with('<') && token.ends_with('>') && token.len() > 2 {
                // Enum-named placeholders keep their literal for the enum
                // expansion below instead of a dummy (no legal dummy
                // may exist).
                if token == "<result>" || token == "<outcome>" || token == "<access>" {
                    for options in &mut token_options {
                        options.push(token.clone());
                    }
                } else {
                    let dummy = dummy_for(&token[1..token.len() - 1]).to_owned();
                    for options in &mut token_options {
                        options.push(dummy.clone());
                    }
                }
            } else {
                for options in &mut token_options {
                    options.push(token.clone());
                }
            }
        }
        for options in token_options {
            // The `<result>` / `<outcome>` placeholders name constrained
            // enums: expand them over every legal value so a renamed
            // variant fails here.
            for argv in expand_enum_placeholders(options) {
                argvs.push((argv, included.clone()));
            }
        }
    }
    argvs
}

/// Every value of a clap `ValueEnum`, by its kebab name.
fn enum_values<T: ValueEnum>() -> Vec<String> {
    T::value_variants()
        .iter()
        .map(|v| {
            v.to_possible_value()
                .expect("enum value has a name")
                .get_name()
                .to_owned()
        })
        .collect()
}

/// Replace placeholders that name constrained enums (`<result>`,
/// `<outcome>`, `<access>`) with every legal value, so the contract pins
/// the vocabulary: a renamed variant fails here instead of in production.
fn expand_enum_placeholders(argv: Vec<String>) -> Vec<Vec<String>> {
    let mut out = vec![argv];
    for (placeholder, values) in [
        ("<result>", enum_values::<FinishResult>()),
        ("<outcome>", enum_values::<Outcome>()),
        ("<access>", enum_values::<PageAccess>()),
    ] {
        let mut next = Vec::new();
        for argv in out {
            if argv.iter().any(|token| token == placeholder) {
                for value in &values {
                    next.push(
                        argv.iter()
                            .map(|token| {
                                if token == placeholder {
                                    value.clone()
                                } else {
                                    token.clone()
                                }
                            })
                            .collect(),
                    );
                }
            } else {
                next.push(argv);
            }
        }
        out = next;
    }
    out
}

/// The failure modes the contract accepts: `--help` short-circuits with
/// `DisplayHelp`, a bare subcommand path (whose syntax lives in another
/// section) reports only missing arguments, and a combination the CLI
/// excludes on purpose (`conflicts_with`, e.g. `--description` with
/// `--description-file`) reports only the conflict.
fn parse_outcome(argv: &[String]) -> Result<(), ParseFailure> {
    let command = Cli::command();
    match command.try_get_matches_from(argv) {
        Ok(_) => Ok(()),
        Err(err) => {
            use clap::error::ErrorKind;
            match err.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => Ok(()),
                ErrorKind::MissingRequiredArgument => {
                    subcommand_path_resolves(argv).map_err(ParseFailure::hard)
                }
                ErrorKind::ArgumentConflict => Err(ParseFailure::conflict(format!("{err}"))),
                _ => Err(ParseFailure::hard(format!("{err}"))),
            }
        }
    }
}

/// A clap rejection: `hard` (an unknown command/flag/value — always a
/// failure) or `conflict` (a combination the CLI excludes on purpose —
/// accepted only when a strict subset of the same variant parses).
#[derive(Debug)]
enum ParseFailure {
    Hard(String),
    Conflict(String),
}

impl ParseFailure {
    fn hard(message: String) -> Self {
        ParseFailure::Hard(message)
    }

    fn conflict(message: String) -> Self {
        ParseFailure::Conflict(message)
    }

    fn message(&self) -> &str {
        match self {
            ParseFailure::Hard(message) | ParseFailure::Conflict(message) => message,
        }
    }
}

/// Every leading non-flag token must resolve as a subcommand path (strict:
/// values are not subcommands, so a span with values must fully parse).
fn subcommand_path_resolves(argv: &[String]) -> Result<(), String> {
    let mut command = Cli::command();
    let mut resolved = 0;
    for token in argv.iter().skip(1) {
        if token.starts_with('-') {
            break;
        }
        match command.find_subcommand(token) {
            Some(next) => {
                command = next.clone();
                resolved += 1;
            }
            None => {
                return Err(format!(
                    "'{}' is not a subcommand of '{}'",
                    token,
                    argv[..=resolved].join(" ")
                ));
            }
        }
    }
    if resolved == 0 {
        return Err("no subcommand path".to_owned());
    }
    Ok(())
}

/// Top-level subcommand names in one argv (after `pidash`).
fn top_level_name(argv: &[String]) -> &str {
    argv.get(1).map(String::as_str).unwrap_or("")
}

/// Assert one candidate parses and teaches an agent (not operator) command.
///
/// A variant rejected with `ArgumentConflict` is accepted when a strict
/// subset of its optional groups parses: the failure is then purely the
/// excluded combination (e.g. `--description` with `--description-file`),
/// while every flag remains individually usable.
fn assert_candidate(kind: &str, stem: &str, raw: &str) {
    let argvs = expand_candidate(raw);
    assert!(
        !argvs.is_empty(),
        "{kind}/{stem}: candidate expands to nothing: {raw}"
    );
    let mut passing: Vec<Vec<usize>> = Vec::new();
    let mut pending: Vec<(Vec<String>, Vec<usize>, ParseFailure)> = Vec::new();
    for (argv, groups) in &argvs {
        assert_eq!(argv.first().map(String::as_str), Some("pidash"));
        match parse_outcome(argv) {
            Ok(()) => passing.push(groups.clone()),
            Err(ParseFailure::Hard(message)) => {
                panic!("{kind}/{stem}: unparsable command {argv:?} from {raw:?}: {message}")
            }
            Err(failure @ ParseFailure::Conflict(_)) => {
                pending.push((argv.clone(), groups.clone(), failure));
            }
        }
        let name = top_level_name(argv);
        // Help and version requests are exempt from the operator refusal
        // (`early_operator_refusal` in super: reading help is never an
        // operation), so a bare `pidash --version` the prompt teaches as a
        // diagnostic needs no Agent classification. Anything else the
        // prompt teaches must be an agent command.
        let help_or_version = argv.iter().any(|arg| {
            arg == "-h" || arg == "--help" || arg == "-V" || arg == "--version" || arg == "help"
        });
        if !help_or_version {
            assert_eq!(
                classify_name(name),
                Some(CommandClass::Agent),
                "{kind}/{stem}: {argv:?} from {raw:?} resolves to a non-agent command"
            );
        }
    }
    for (argv, groups, failure) in pending {
        // Every subset is asserted independently, so a hard error (an
        // unknown flag, a bad value) fails the test on its own variant:
        // only pure combination exclusions reach this rule.
        let subset_passes = passing.iter().any(|passed| {
            passed.len() < groups.len() && passed.iter().all(|group| groups.contains(group))
        });
        if !subset_passes {
            panic!(
                "{kind}/{stem}: conflicting command {argv:?} from {raw:?} has no passing subset: {}",
                failure.message()
            );
        }
    }
}

/// Rendered `## `–`#### ` headings (exact text, Jinja already rendered).
fn rendered_headings(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let stripped = line.trim_start();
            if stripped.starts_with("#### ")
                || stripped.starts_with("### ")
                || stripped.starts_with("## ")
            {
                Some(stripped.trim_start_matches('#').trim().to_owned())
            } else {
                None
            }
        })
        .collect()
}

/// Quoted section references: `see "…"`, `as "…"`, `comes from "…"`,
/// `set by "…"`. Other quoted prose (`during "…"`, outcome names in
/// backticked commands) is not a section pointer.
fn section_references(text: &str) -> Vec<String> {
    let mut refs = Vec::new();
    for line in text.lines() {
        let lower = line.to_lowercase();
        for marker in ["see \"", "as \"", "comes from \"", "set by \""] {
            let mut from = 0;
            while let Some(found) = lower[from..].find(marker) {
                let start = from + found + marker.len();
                if let Some(end) = line[start..].find('"') {
                    let reference = line[start..start + end].to_owned();
                    if !refs.contains(&reference) {
                        refs.push(reference);
                    }
                    from = start + end + 1;
                } else {
                    break;
                }
            }
        }
    }
    refs
}

#[test]
fn every_prompt_command_parses() {
    for (kind, stem, text) in fixture_texts() {
        let candidates = extract_candidates(&text);
        assert!(
            !candidates.is_empty(),
            "{kind}/{stem}: no pidash commands extracted"
        );
        for raw in &candidates {
            assert_candidate(kind, &stem, raw);
        }
    }
}

#[test]
fn handoff_file_is_coding_task_only() {
    for (kind, stem, text) in fixture_texts() {
        // The lifecycle-owned syntax line carries `--handoff-file` exactly
        // on coding-task renders (matched on the invocation form, not on
        // prose that merely names the command).
        let finish_lines: Vec<&str> = text
            .lines()
            .filter(|line| line.starts_with("    ") && line.contains("pidash run finish --to"))
            .collect();
        if kind != "scheduler" {
            assert!(
                !finish_lines.is_empty(),
                "{kind}/{stem}: no run-finish syntax line rendered"
            );
        }
        for line in &finish_lines {
            assert_eq!(
                line.contains("--handoff-file"),
                kind == "coding-task",
                "{kind}/{stem}: {line}"
            );
        }
        // … and no other render mentions the flag at all.
        if kind != "coding-task" {
            assert!(
                !text.contains("handoff-file"),
                "{kind}/{stem} mentions --handoff-file"
            );
        }
    }
}

#[test]
fn section_references_match_rendered_headings() {
    for (kind, stem, text) in fixture_texts() {
        let headings = rendered_headings(&text);
        assert!(!headings.is_empty(), "{kind}/{stem}: no headings rendered");
        for reference in section_references(&text) {
            assert!(
                headings.contains(&reference),
                "{kind}/{stem}: see \"{reference}\" matches no rendered heading"
            );
        }
    }
}

/// `PIDASH_*` names a render promises, per kind.
fn promised_env_vars(kind: &str, text: &str) -> BTreeMap<String, ()> {
    let mut vars = BTreeMap::new();
    let mut rest = text;
    while let Some(start) = rest.find("PIDASH_") {
        let tail = &rest[start + "PIDASH_".len()..];
        let len = tail
            .char_indices()
            .take_while(|(_, ch)| ch.is_ascii_uppercase() || ch.is_ascii_digit() || *ch == '_')
            .map(|(i, _)| i)
            .last()
            .map(|i| i + 1)
            .unwrap_or(0);
        if len > 0 {
            vars.insert(format!("PIDASH_{}", &tail[..len]), ());
        }
        rest = &tail[len.min(tail.len())..];
    }
    // The scheduler prompt names no issue, so it must not promise one.
    if kind == "scheduler" {
        assert!(
            !vars.contains_key("PIDASH_ISSUE_IDENTIFIER"),
            "scheduler promises an issue identifier"
        );
    }
    vars
}

#[tokio::test]
async fn promised_env_vars_are_set_for_the_kind() {
    for (kind, stem, text) in fixture_texts() {
        let issue_run = kind != "scheduler";
        let env = RunEnv {
            run_id: uuid::Uuid::new_v4(),
            backend: BackendKind::None,
            runner_bin_dir: None,
            config_dir: None,
            data_dir: None,
            runner_exe: None,
            issue_identifier: issue_run.then(|| "SAMPLE-1".to_owned()),
            project_identifier: Some("SAMPLE".to_owned()),
            agent_name: Some("Sample Agent (sample-model)".to_owned()),
        };
        let vars = promised_env_vars(kind, &text);
        assert!(
            !vars.is_empty(),
            "{kind}/{stem}: prompt promises no PIDASH_* var"
        );
        RUN_ENV
            .scope(env.clone(), async {
                let command = crate::util::shell::login_shell_command("true", &[], None);
                let applied: BTreeMap<String, String> = command
                    .as_std()
                    .get_envs()
                    .filter_map(|(key, value)| {
                        Some((
                            key.to_string_lossy().into_owned(),
                            value?.to_string_lossy().into_owned(),
                        ))
                    })
                    .collect();
                for var in vars.keys() {
                    assert!(
                        applied.contains_key(var),
                        "{kind}/{stem}: prompt promises {var} but apply_run_env does not set it"
                    );
                }
                assert_eq!(
                    applied.get("PIDASH_RUN_ID").map(String::as_str),
                    Some(env.run_id.to_string()).as_deref(),
                    "{kind}/{stem}: PIDASH_RUN_ID"
                );
                if issue_run {
                    assert_eq!(
                        applied.get("PIDASH_ISSUE_IDENTIFIER").map(String::as_str),
                        Some("SAMPLE-1"),
                        "{kind}/{stem}: PIDASH_ISSUE_IDENTIFIER"
                    );
                } else {
                    assert!(
                        !applied.contains_key("PIDASH_ISSUE_IDENTIFIER"),
                        "{kind}/{stem}: scheduler run must not set PIDASH_ISSUE_IDENTIFIER"
                    );
                }
                assert_eq!(
                    applied.get("PIDASH_PROJECT").map(String::as_str),
                    Some("SAMPLE"),
                    "{kind}/{stem}: PIDASH_PROJECT"
                );
                // Attribution defaults (`comment add`, `run finish`) read
                // the agent name: A exports it for issue and scheduler
                // runs alike, and the prompt relies on it.
                assert_eq!(
                    applied.get("PIDASH_AGENT_NAME").map(String::as_str),
                    Some("Sample Agent (sample-model)"),
                    "{kind}/{stem}: PIDASH_AGENT_NAME"
                );
            })
            .await;
    }
}

#[test]
fn docs_topics_parse() {
    for topic in TOPICS {
        let text = topic_text(topic)
            .unwrap_or_else(|| panic!("docs topic {topic} renders"))
            .to_owned();
        assert!(!text.is_empty(), "docs topic {topic} is empty");
        for raw in extract_candidates(&text) {
            assert_candidate("docs", topic, &raw);
        }
    }
}

/// The contract fails loudly on a command the CLI cannot parse (shown once
/// with a bad fixture during NEWPIDASH-325; kept as a regression pin).
#[test]
fn missing_fixture_proof() {
    let bad = "pidash comment list --sincee 2026-01-01T00:00:00Z";
    let argvs = expand_candidate(bad);
    assert_eq!(argvs.len(), 1);
    assert!(parse_outcome(&argvs[0].0).is_err());
}
