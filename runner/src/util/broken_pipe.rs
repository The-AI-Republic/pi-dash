// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Quiet exit when a one-shot command's stdout reader goes away.
//!
//! Rust ignores SIGPIPE at startup, so a write to a closed pipe returns
//! `EPIPE` and `println!` turns that into a panic ("failed printing to
//! stdout: Broken pipe"). Scripts that run `pidash … | head` then see a
//! panic on stderr and a non-zero exit.
//!
//! Resetting SIGPIPE to its default action would need `unsafe` (the crate
//! forbids it), and wrapping every `println!` would leave any new one
//! unprotected. Instead [`install`] adds a panic hook that recognises std's
//! stdout-print panic when its cause is a broken pipe and exits with 141
//! (128 + SIGPIPE, what a shell reports for `yes | head`) without printing
//! anything. Every other panic goes to the previous hook unchanged.
//!
//! The hook is process-wide, so it is installed only for one-shot CLI
//! commands, never for the long-running daemon (`pidash __run`).

use std::io::{self, ErrorKind};
use std::panic::PanicHookInfo;

/// Exit status used for a broken stdout pipe: 128 + SIGPIPE (13).
pub const EXIT_BROKEN_PIPE: i32 = 141;

/// The prefix std puts on the panic raised by `print!` / `println!`
/// (`library/std/src/io/stdio.rs`: `"failed printing to {label}: {e}"`).
const STDOUT_PANIC_PREFIX: &str = "failed printing to stdout: ";

/// Install the broken-pipe hook in front of the current panic hook.
pub fn install() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if is_stdout_broken_pipe(info) {
            std::process::exit(EXIT_BROKEN_PIPE);
        }
        previous(info);
    }));
}

fn is_stdout_broken_pipe(info: &PanicHookInfo<'_>) -> bool {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied());
    message.is_some_and(is_stdout_broken_pipe_message)
}

/// Whether `message` is std's stdout-print panic caused by a broken pipe.
///
/// The cause is the `Display` of the `io::Error`, so it is compared against
/// the same rendering of every OS error that maps to
/// [`ErrorKind::BrokenPipe`] on this platform; that keeps the check exact
/// without depending on the error text of any one OS.
pub fn is_stdout_broken_pipe_message(message: &str) -> bool {
    let Some(cause) = message.strip_prefix(STDOUT_PANIC_PREFIX) else {
        return false;
    };
    broken_pipe_errors().any(|err| err.to_string() == cause)
}

fn broken_pipe_errors() -> impl Iterator<Item = io::Error> {
    // EPIPE on Linux / macOS / BSD; ERROR_BROKEN_PIPE and ERROR_NO_DATA on
    // Windows. Only codes that this platform maps to BrokenPipe count.
    [32, 109, 232]
        .into_iter()
        .map(io::Error::from_raw_os_error)
        .filter(|err| err.kind() == ErrorKind::BrokenPipe)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn broken_pipe_cause() -> String {
        broken_pipe_errors()
            .next()
            .expect("platform has a broken-pipe OS error")
            .to_string()
    }

    #[test]
    fn matches_stds_stdout_broken_pipe_panic() {
        let message = format!("{STDOUT_PANIC_PREFIX}{}", broken_pipe_cause());
        assert!(is_stdout_broken_pipe_message(&message));
    }

    #[test]
    fn ignores_other_stdout_errors() {
        let cause = io::Error::from(ErrorKind::PermissionDenied);
        let message = format!("{STDOUT_PANIC_PREFIX}{cause}");
        assert!(!is_stdout_broken_pipe_message(&message));
    }

    #[test]
    fn ignores_broken_pipe_on_stderr() {
        let message = format!("failed printing to stderr: {}", broken_pipe_cause());
        assert!(!is_stdout_broken_pipe_message(&message));
    }

    #[test]
    fn ignores_unrelated_panics() {
        assert!(!is_stdout_broken_pipe_message(
            "called `Option::unwrap()` on a `None` value"
        ));
    }
}
