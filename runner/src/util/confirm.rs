//! Tiny y/N prompt for destructive CLI subcommands.
//!
//! Hand-rolled rather than pulled in from `dialoguer` to avoid a new
//! dep on a single-use surface. The CLI only confirms a handful of
//! actions (today: `pidash runner remove`); a full prompt crate is
//! overkill.
//!
//! Behaviour:
//! - Empty input (just <Enter>) returns the supplied default.
//! - "y" / "yes" (case-insensitive) returns true.
//! - "n" / "no" returns false.
//! - Anything else re-prompts up to a small retry limit, then bails.
//! - EOF / closed stdin returns the default — matches the way Unix
//!   `rm -i </dev/null` behaves.
//!
//! Subcommands that need to skip the prompt (CI, scripted use) pass
//! `--yes`/`-y` and call [`maybe_confirm`] with `assume_yes=true`.

use std::io::{self, BufRead, Write};

const MAX_RETRIES: usize = 3;

/// Print the prompt and read a yes/no answer from stdin. See module
/// docs for behaviour.
pub fn ask(prompt: &str, default: bool) -> bool {
    ask_with_streams(prompt, default, &mut io::stdin().lock(), &mut io::stderr())
}

/// Same as [`ask`] but accepts injectable streams so tests can drive
/// the prompt without a real TTY.
pub fn ask_with_streams<R: BufRead, W: Write>(
    prompt: &str,
    default: bool,
    input: &mut R,
    output: &mut W,
) -> bool {
    let suffix = if default { "[Y/n]" } else { "[y/N]" };
    for _ in 0..MAX_RETRIES {
        let _ = write!(output, "{prompt} {suffix} ");
        let _ = output.flush();
        let mut line = String::new();
        match input.read_line(&mut line) {
            Ok(0) => return default,
            Ok(_) => {}
            Err(_) => return default,
        }
        let trimmed = line.trim().to_ascii_lowercase();
        match trimmed.as_str() {
            "" => return default,
            "y" | "yes" => return true,
            "n" | "no" => return false,
            _ => {
                let _ = writeln!(output, "Please answer 'y' or 'n'.");
            }
        }
    }
    let _ = writeln!(output, "Too many invalid responses; assuming 'no'.");
    false
}

/// Skip the prompt entirely when `assume_yes` is set.
///
/// Convenience wrapper for subcommands that expose a `--yes`/`-y` flag.
/// Returns `true` immediately if the flag is set, otherwise delegates
/// to [`ask`].
pub fn maybe_confirm(prompt: &str, default: bool, assume_yes: bool) -> bool {
    if assume_yes {
        return true;
    }
    ask(prompt, default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn run(input: &str, default: bool) -> (bool, String) {
        let mut cursor = Cursor::new(input.as_bytes().to_vec());
        let mut out: Vec<u8> = Vec::new();
        let answer = ask_with_streams("proceed?", default, &mut cursor, &mut out);
        (answer, String::from_utf8(out).unwrap())
    }

    #[test]
    fn empty_returns_default_true() {
        let (ans, _) = run("\n", true);
        assert!(ans);
    }

    #[test]
    fn empty_returns_default_false() {
        let (ans, _) = run("\n", false);
        assert!(!ans);
    }

    #[test]
    fn y_returns_true_regardless_of_default() {
        let (ans, _) = run("y\n", false);
        assert!(ans);
        let (ans, _) = run("YES\n", false);
        assert!(ans);
    }

    #[test]
    fn n_returns_false_regardless_of_default() {
        let (ans, _) = run("n\n", true);
        assert!(!ans);
        let (ans, _) = run("No\n", true);
        assert!(!ans);
    }

    #[test]
    fn invalid_then_valid_succeeds_with_reprompt_message() {
        let (ans, out) = run("maybe\ny\n", false);
        assert!(ans);
        assert!(out.contains("Please answer"));
    }

    #[test]
    fn three_invalid_responses_assumes_no() {
        let (ans, out) = run("a\nb\nc\nd\n", true);
        assert!(
            !ans,
            "after MAX_RETRIES invalid responses, must default to no"
        );
        assert!(out.contains("Too many invalid"));
    }

    #[test]
    fn eof_returns_default() {
        let (ans, _) = run("", true);
        assert!(ans);
        let (ans, _) = run("", false);
        assert!(!ans);
    }

    #[test]
    fn maybe_confirm_short_circuits_when_assume_yes() {
        // `assume_yes=true` returns true without reading input.
        assert!(maybe_confirm("ignored", false, true));
    }
}
