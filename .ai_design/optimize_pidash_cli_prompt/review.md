# Design review: the `pidash` CLI part of the agent prompt

> Companion to `design.md` in this directory. Where the two differ, **`design.md`
> is authoritative** (e.g. the workpad file moved to `.pidash/workpad.md` and
> the stale guard uses a content hash; see design §6.1 and §10).
>
> Reviewed 2026-11-18 against
> `main` @ `e365e3c62` and the `pidash 0.1.31` test-runner binary.
>
> **Method:**
>
> - read `apps/api/pi_dash/prompting/sections/pidash-cli.md` line by line;
> - checked every documented command and flag (42 command paths) against
>   `pidash <cmd> --help`;
> - checked every documented environment variable against what the runner
>   actually sets (`runner/src/util/shell.rs`, `runner/src/api_client.rs`);
> - replayed the PIDASHCLOU-2 transcripts to see which steps were caused by
>   the prompt.

## Summary

The CLI reference is accurate at the flag level: 41 of 42 command paths
match the binary. Its problems are elsewhere:

1. **The prompt tells agents to re-fetch data it already contains.** That
   cost more on PIDASHCLOU-2 than the manual's size did.
2. **The environment contract is wrong.** It describes variables the runner
   doesn't set and a credential model that no longer exists.
3. **Bookkeeping the CLI could do itself is pushed onto the agent.** Every
   comment needs two flags the runner already knows.
4. Then size: about 60% of the section is reference material a typical run
   never touches.

Findings are ordered by impact.

---

## R1. Agents re-fetch what the prompt already inlined (highest impact)

The composer already inlines the issue, the comments to date, the project
states (`### Available states`) and, for review/test, the workpad body
(`workpad-context.md`; `context.py:650` says it is inlined to "save an extra
`pidash workpad get` round-trip"). The CLI section then tells the agent to
fetch them again:

- `pidash-cli.md:68`, `comment list`: "Read these in chronological order to
  pick up any human replies since your last run."
- `pidash-cli.md:156`, recipe: "**Always start each edit by re-fetching from
  the server**": `pidash workpad get | jq -r .body > ./.pidash-workpad.md`.
- `test-cycle.md:41`: "read it with `pidash workpad get`."

What the agents did on PIDASHCLOU-2 (26 API calls in total):

| Run       | Redundant or failed calls                                                                                                               |
| --------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| Implement | `workpad get` bare → error (NEWPIDASH-317)                                                                                              |
| Review    | `comment list` + `issue get` (both inlined); `state list` + `workpad get` → error; retry of `state list` + `workpad get` (both inlined) |
| Test      | `comment list` (inlined); `workpad get` → error; retry `workpad get` (inlined)                                                          |

That's about 7 of 26 calls. Each re-sends ~55–70k tokens, so **~420k tokens
(~25% of the issue) went to fetching things already in context.** That is
more than shrinking the manual saves (~200k).

**Fix:**

- **The runner writes the workpad to `./.pidash-workpad.md` before starting
  the agent.** Today the body reaches the runner only inside the rendered
  prompt; add it as a separate field on the assignment so the runner can
  write the file. The prompt says "your
  workpad is in `./.pidash-workpad.md` and shown below; edit it and run
  `pidash workpad update`." The re-fetch rule then only applies to _second
  and later_ edits in the same run (the file is deleted after a successful
  update). That should also become cheaper: make `--keep` the default and
  rely on `updated_at` for conflict detection instead of deleting the file.
- **Say what is fresh:** "The issue, comments, states and workpad below were
  read when this run started (`{{ rendered_at }}`). Re-fetch only if you
  have been working long enough that a human may have replied, or before an
  irreversible move."
- Remove "Read these… to pick up any human replies" from `comment list`.
  Keep the command in the index.
- Make `test-cycle.md:41` refer to the inlined workpad.

## R2. The environment contract is wrong

`pidash-cli.md:10-20` says the CLI reads `PIDASH_API_URL`,
`PIDASH_WORKSPACE_SLUG`, `PIDASH_TOKEN` (a "session-scoped credential"),
`PIDASH_ISSUE_IDENTIFIER` and (scheduler) `PIDASH_PROJECT` from the
environment.

What the code does:

- The runner sets exactly one of these on the agent: `PIDASH_RUN_ID`
  (`shell.rs` `apply_run_env`).
- The CLI resolves URL, workspace and token from the **runner config file**
  first (`api_client.rs` `CliEnv::resolve`, since #109). Env vars are only a
  fallback.
- The token is the **runner owner's own CLI token**, written by
  `pidash auth login`. `api_client.rs:86-91` says so: "the token resolves to
  the runner owner's account". It is not session-scoped.
- `PIDASH_ISSUE_IDENTIFIER` and `PIDASH_PROJECT` are never set
  (NEWPIDASH-317 covers the first; nobody covers the second).

Effects: bare commands fail (R1, NEWPIDASH-317). The agent also gets a false
picture of its blast radius: it acts with the full permissions of the person
who owns the runner, while the prompt says the credential is
session-scoped. That matters for the "All writes are real and immediate"
convention, and for anyone reasoning about prompt-injection risk.

**Fix:** rewrite the subsection to match reality: "`pidash` is already
authenticated **as the runner's owner** (`{{ owner }}`); your writes carry
their identity plus this run's id." Export `PIDASH_ISSUE_IDENTIFIER`
(NEWPIDASH-317) and `PIDASH_PROJECT` for scheduler runs. Drop the three
connection variables from the prompt; the agent never needs them.

## R3. Scheduler prompt documents a flag that doesn't exist

`pidash-cli.md:118` renders, for scheduler runs:
`pidash state list --project <PROJ>`. `pidash state list` has **no
`--project` flag**. It takes one positional argument that must be a
**project UUID or an issue identifier**, not a project identifier like
`ENG` (`runner/src/cli/state.rs`, `StateCommand::List`). Every scheduler run
that follows the prompt gets a clap error. It was the only mismatch among
the 42 command paths checked.

**Fix:** either add `--project <PROJ-or-UUID>` to `state list`, accepting
identifiers like `issue list --project` does (preferred: consistent with
every other project-scoped command), or render the project UUID positionally
in the scheduler prompt. Add a test that runs every command line in the
rendered prompts through clap parsing (see R9).

## R4. Comment attribution is manual bookkeeping the CLI could do

`pidash-cli.md:69` requires `--as-agent "<agent name>" --agent-run-id
"{{ run.id }}"` on every comment, and asks the agent to guess its own
runtime name ("use your actual runtime name if you know it… otherwise use
`AI Agent`"). Meanwhile:

- the CLI already has the run id (`CliEnv.run_id` from `PIDASH_RUN_ID`) and
  sends it as `X-Pi-Dash-Run-Id` on every write;
- the runner knows the agent kind (`[runner.agent] kind`);
- `comment add` doesn't default either flag (`comment.rs`
  `add_speaker_metadata`). A comment posted without them is attributed to
  the human owner, and scheduler run tracing (`scheduler.py:151-171`, which
  finds the issues a run touched by `speaker_agent_run_id`) silently misses
  it.

**Fix:** the runner exports `PIDASH_AGENT_NAME` (from agent kind and model);
`comment add` defaults `--as-agent` / `--agent-run-id` from
`PIDASH_AGENT_NAME` / `PIDASH_RUN_ID` when running inside a run. (Server-side
inference from the header is the stronger version: then even a bare `curl`
can't misattribute.) The prompt line becomes
`pidash comment add <id> --body-file <path> [--fold]`. This also removes
`{{ run.id }}` from three places in the section (R8).

## R5. About 60% of the section is reference a typical run never uses

Rendered issue-run section ≈ 34 KB. Rough split:

| Block                                                                                                                                 | Size    | Typical issue run uses it? |
| ------------------------------------------------------------------------------------------------------------------------------------- | ------- | -------------------------- |
| Schedulers (11 commands, admin-only writes)                                                                                           | ~7.5 KB | almost never               |
| Issues: list filters, create, relate/unrelate, relations, search (incl. a backwards-compat note for runners too old to have `search`) | ~7 KB   | sometimes                  |
| Pods and dedicated runners                                                                                                            | ~3.5 KB | rarely                     |
| Pages                                                                                                                                 | ~3 KB   | rarely                     |
| Labels (+ attach examples)                                                                                                            | ~3.5 KB | rarely                     |
| Core: env, output contract, workpad, comment add/list, issue get/patch, run yield, not-for-you, conventions, states                   | ~9 KB   | always                     |

This is `design.md` §6.1–6.2 (core inline, the rest via `pidash docs`).
Additional notes:

- **Schedulers.** Issue runs never need the eleven scheduler commands. Show
  them only in scheduler recipes (one `{% if %}`), and as a single line in the
  index for issue runs.
- **`issue search` backwards-compat note (~600 B).** It handles runners
  older than the subcommand. Enforce a minimum runner version for prompts
  instead of teaching every run about old binaries.
- **Pods/dedicated runners.** Most of this is policy for humans or triage
  agents ("leave an issue on its pod…"). Keep a one-line rule inline; move
  the rest to docs.

## R6. "Not for you" list is stale

`pidash-cli.md:152` lists `configure` and `rotate`, which no longer exist,
and misses `runner` (incl. `runner add`, which rewrote and restarted the
live service on 2026-11-16 when a test ran it), `update`, `engine`,
`reconnect`, `login`, `auth`, `config`. It also misses `ai`, which calls the
cloud AI assistant on the user's BYOK key, so it costs money and needs
explicit permission at least. Fix:

- generate this list from the CLI (a `#[command(hide_in_run)]`-style marker or
  a static list in `cli/mod.rs` with a unit test that every top-level
  subcommand is classified agent/operator);
- render it into the prompt from that one source;
- hide those commands from `--help` inside a run (`design.md` §6.5); consider
  refusing them when `PIDASH_RUN_ID` is set.

## R7. The same flows are re-specified in several places

The stage-ending flow is given three times per prompt: as recipes in
`pidash-cli.md:164-193`, in the stage's own cycle section ("Step 3 — Decide
where the task goes next"), and in `ending-run.md`. The `run yield` outcome
semantics and `--stop-ticking` rules appear in `pidash-cli.md:138` and again
in `ending-run.md:10-36` (`ending-run` is the more complete one). The retry
rule appears at `pidash-cli.md:32` and again at `:216`. Copies drift: the
Django/Rust split (R10) already shows it.

**Fix:** one owner per fact:

- CLI section: syntax of each core command, one line of semantics.
- `ending-run`: outcomes and clock rules.
- Cycle sections: which state to move to.
- Delete the stage-ending recipes from `pidash-cli.md` (they are the third
  copy) and the duplicate retry rule.

## R8. Per-run values are scattered through static text

`{{ run.id }}`, `{{ issue.identifier }}` and `run.kind` branches are
interleaved throughout the section, so the rendered text differs per run
from the first variable onward. Two consequences:

- It blocks moving the static part of the prompt somewhere it can be cached
  across runs. With Claude Code, for example, static text in
  `--append-system-prompt` would sit in the system-prompt prefix that is
  already cached across sessions (the 13k-token cross-run cache hit seen on
  PIDASHCLOU-2). Today every run _writes_ ~36–45k tokens of new prompt cache
  at the 1-hour price on its first call.
- It makes the section harder to test: every `run.kind` permutation is a
  different document.

**Fix:** collect per-run values into one small "This run" block at the top
(issue id, run id, stage, agent name). Static sections refer to "this issue"
/ "this run" instead of templating ids inline. R4 removes most `{{ run.id }}`
uses anyway. Whether to then move static sections into a system-prompt
channel is per-agent-bridge work; evaluate it separately (it changes how
`runner/src/bridges/*` launch agents).

## R9. No test checks that the prompt is runnable

R3 and the `PIDASH_ISSUE_IDENTIFIER` gap (NEWPIDASH-317) shipped because
nothing checks the prompt against the binary. Add a runner-side test that:

1. renders each recipe kind with a fixture context (or takes rendered
   fixtures from the API tests);
2. extracts every `pidash …` command line;
3. parses each one with the real clap `Command` (`try_get_matches_from`),
   substituting placeholders;
4. asserts every `PIDASH_*` variable the prompt mentions is either set by
   `apply_run_env` or explicitly documented as optional.

The 42-path check used for this review is a crude version of step 3 and took
a few seconds.

## R10. Django and Rust copies have drifted

`rust-api/crates/services/src/prompting/registry.rs` claims the Rust
`sections/*.md` are byte-identical mirrors. `pidash-cli.md` and
`workpad-setup.md` differ. The Rust copy has the newer `--allow-empty` /
`set -o pipefail` text; the Django copy still says "An empty file clears
it", which is **wrong** for the current binary (it refuses empty bodies
without `--allow-empty`). Add a CI diff of the two trees, and sync before
any change from this review lands.

---

## Recommended order

| #   | Change                                                          | Effort | Saves / fixes                       |
| --- | --------------------------------------------------------------- | ------ | ----------------------------------- |
| 1   | R10 sync + CI diff                                              | S      | correctness of everything below     |
| 2   | NEWPIDASH-317 + R3 (`state list --project`)                     | S      | guaranteed-failure steps            |
| 3   | R1 pre-written workpad file + "already inlined, don't re-fetch" | S–M    | ~25% of tokens on PIDASHCLOU-2      |
| 4   | R4 default comment attribution                                  | S      | prompt text, misattributed comments |
| 5   | R2 rewrite environment contract                                 | S      | correctness and security model      |
| 6   | R6 generated blocklist                                          | S      | safety                              |
| 7   | R9 prompt-vs-CLI test                                           | M      | stops regressions of 2, 4, 6        |
| 8   | R5 + R7 core/index/docs split, de-duplication (`design.md` §6)  | M      | ~7–8k tokens per step               |
| 9   | R8 per-run block (+ evaluate system-prompt channel)             | M–L    | cache cost                          |

Items 1–6 are small and mostly independent. Item 3 alone is worth more than
the manual split in item 8.
