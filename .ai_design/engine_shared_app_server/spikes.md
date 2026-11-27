# Spikes S1–S4: shared engine app-server (NEWPIDASH-333, part of NEWPIDASH-328)

Design: `design/engine-shared-app-server` branch (PR #3409),
`.ai_design/engine_shared_app_server/design.md` §8. Pinned engine under test:
`runner/ENGINE_VERSION` = `rust-v0.153.4`, binary reports `codex-cli 0.153.4`.
Probes drove the engine over stdio JSON-RPC directly (no model turns: no model
credential in this environment; model TTFT is unchanged by the design either way).

## S1 — one app-server, one connection, several threads: YES (with one caveat)

- `initialize` + `initialized` once per process (0.35 s first call here).
- Two `thread/start` with different `cwd`/`sandbox`/`approvalPolicy`
  (`workspace-write`/`on-request` vs `read-only`/`on-request`) returned distinct
  thread ids (`01a1f7bb-…94ac2`, `01a1f7bb-…3de47`). The response echoes the
  thread record including `cwd`, `model`, `historyMode: "paginated"`.
- `thread/unsubscribe` exists on the pinned engine: `{"status": "unsubscribed"}`.
- Caveat — `thread/resume` (`{"threadId": …}`, the shape `bridge.rs`
  `ThreadResumeParams` sends) failed with
  `{"code": -32600, "message": "no rollout found for thread id …"}` both
  in-process and on a fresh process with the same `CODEX_HOME`. The probed
  threads had run zero turns, and the engine wrote no rollout file for them
  (the `sessions/2026/11/19/` dir did not exist afterwards), so there was
  nothing to resume. Consequence for the build: resume-after-crash works for
  threads that have completed at least one turn; zero-turn threads must fall
  back to `thread/start` — which is exactly the fallback `Bridge::warm`
  already implements. No design change; the `EngineChatServer` warm path keeps
  resume-then-start. Turn-level concurrency (two `turn/start` at once, `threadId`
  on notifications/approvals) still needs a credentialed run and will be proven
  by the fake-server tests + a credentialed pass, not claimed here.

## S2 — new/resumed thread picks up a changed `config.toml`: YES for new threads

- With `model = "gpt-5.1-codex-mini"` in `CODEX_HOME/config.toml`, `thread/start`
  echoed `"model": "gpt-5.1-codex-mini"`; after rewriting the file to
  `model = "gpt-5.1-codex-max"` (no restart), the next `thread/start` echoed
  `"model": "gpt-5.1-codex-max"`. New threads read the current config.
- Resumed threads could not be probed (see S1 caveat). MVP keeps the decided
  behaviour (O4: restart when idle on profile change); this result only means a
  future optimisation could skip the restart for threads started after the change.

## S3 — concurrent writers to `CODEX_HOME/config.toml`: last-writer-wins, self-healing

Three writers share the file:

1. the engine appends `[projects."<cwd>"] trust_level = "trusted"` on
   `thread/start` (observed: the probe's `config.toml` gained the table right
   after the two starts);
2. the daemon's profile refresh (`write_engine_config` in
   `runner/src/pidash_agent/codex_config.rs`) renders the whole file and
   replaces it atomically (`atomic_write`: temp file + rename) — the renderer
   has no `projects`/`trust` handling, so a refresh **drops** trust markings;
3. per-run engine processes (`AgentRun`) share the same `CODEX_HOME` and mark
   trust the same way.
   All writes are atomic renames, so the file is never torn — but a refresh can
   drop a marking another writer just added, and a marking can land on a config a
   refresh is about to replace. Impact is low: the marking is re-added silently on
   the next `thread/start` in that `cwd` (the probe was never prompted). No design
   change for MVP; if trust-flap ever prompts users, the fix is a
   merge-preserving rewrite (read-modify-write of the `model`/`catalog` keys only).

## S4 — baseline: time to thread-ready, per-session process vs shared server

`spawn + initialize + thread/start` (the cost the shared server moves off the
chat path), 3 samples each on this machine (warm page cache):

- fresh process per chat (today): 0.161 s, 0.122 s, 0.121 s (mean 0.135 s)
- extra `thread/start` on a warm shared server: 0.020 s × 3 (mean 0.020 s)
- saved per chat: ~0.115 s to thread-ready, plus one fewer process per chat
  (today up to 8 live; the reaper logic goes away with the per-session bridge).

This is to thread-ready, not to first reply token: model TTFT is identical
before/after and needs credentials to measure. End-to-end TTFR will be
re-measured with credentials before merge and posted on the issue.

## Design changes required by the spikes

None to §4.1–4.6, O4 or O5. Two build notes (already reflected above):

- Warm keeps resume-then-`thread/start` (S1 caveat makes the fallback load-bearing
  for zero-turn threads, not just unknown ids).
- No merge-preserving config rewrite for MVP (S3 self-heals).
