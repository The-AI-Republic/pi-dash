//! MR-11 (PDASHOSS01-119): the managed-run creation events reach a real
//! `fmt` subscriber at the binary's default `info` level — the Rust half of
//! the Python "delivered to a sink, not just to caplog" criterion — and
//! carry only ids.

use std::io::Write;
use std::sync::{Arc, Mutex};

use pidash_services::dispatch::events::log_managed_run_created;
use pidash_types::dispatch::AgentExecutorKind;
use uuid::Uuid;

#[derive(Clone, Default)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl Write for Buf {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("buf").extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Run `emit` under the binary's default level (`EnvFilter` falls back
/// to `info`, `bin/pidash-api/src/main.rs`) and return what it wrote.
fn captured(emit: impl FnOnce()) -> String {
    let buf = Buf::default();
    let writer = buf.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, emit);
    let bytes = buf.0.lock().expect("buf").clone();
    String::from_utf8(bytes).expect("utf8")
}

#[test]
fn run_pinned_is_delivered_with_run_and_runner() {
    let run = Uuid::from_u128(1);
    let runner = Uuid::from_u128(2);
    let out = captured(|| {
        log_managed_run_created(run, AgentExecutorKind::ManagedRunner, "", Some(runner))
    });
    assert!(out.contains("INFO"), "{out}");
    assert!(out.contains("managed_runner.run_pinned"), "{out}");
    assert!(out.contains(&format!("run={run}")), "{out}");
    assert!(out.contains(&format!("runner={runner}")), "{out}");
    assert!(!out.contains("queued_waiting"), "{out}");
}

#[test]
fn queued_waiting_is_delivered_for_a_parked_run() {
    let run = Uuid::from_u128(3);
    let runner = Uuid::from_u128(4);
    let out = captured(|| {
        log_managed_run_created(
            run,
            AgentExecutorKind::ManagedRunner,
            "desktop_not_connected",
            Some(runner),
        )
    });
    assert!(out.contains("managed_runner.queued_waiting"), "{out}");
    assert!(out.contains(&format!("run={run}")), "{out}");
    assert!(out.contains(&format!("runner={runner}")), "{out}");
    assert!(!out.contains("run_pinned"), "{out}");
}

#[test]
fn non_managed_runs_log_nothing() {
    let out = captured(|| {
        for kind in [
            AgentExecutorKind::LocalRunner,
            AgentExecutorKind::CloudAgent,
        ] {
            log_managed_run_created(Uuid::from_u128(5), kind, "", Some(Uuid::from_u128(6)));
            log_managed_run_created(Uuid::from_u128(5), kind, "desktop_not_connected", None);
        }
    });
    assert_eq!(out, "");
}

#[test]
fn events_carry_no_token_or_home_path() {
    let out = captured(|| {
        log_managed_run_created(
            Uuid::from_u128(7),
            AgentExecutorKind::ManagedRunner,
            "",
            Some(Uuid::from_u128(8)),
        );
        log_managed_run_created(
            Uuid::from_u128(9),
            AgentExecutorKind::ManagedRunner,
            "desktop_not_connected",
            None,
        );
    });
    assert_eq!(out.lines().count(), 2, "{out}");
    for needle in ["token", "/home/", "/Users/", "~/"] {
        assert!(!out.contains(needle), "{needle} in {out}");
    }
}
