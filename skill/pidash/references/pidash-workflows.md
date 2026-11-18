# Pi Dash Agent Workflows

Detailed reference for the `pidash` CLI as a coding agent uses it. Exact flags come from `pidash <command> <subcommand> --help`; when this file and `--help` disagree, `--help` wins.

## Contents

- [Preflight](#preflight)
- [Output, errors, and exit codes](#output-errors-and-exit-codes)
- [Project selection](#project-selection)
- [Reading issues](#reading-issues)
- [Search](#search)
- [Listing](#listing)
- [Creating issues](#creating-issues)
- [Updating issues](#updating-issues)
- [Labels](#labels)
- [Relations and blockers](#relations-and-blockers)
- [Comments](#comments)
- [Pull requests and merge requests](#pull-requests-and-merge-requests)
- [Pages](#pages)
- [Runners, pods, and agent runs](#runners-pods-and-agent-runs)
- [Schedulers](#schedulers)
- [Asking the cloud assistant](#asking-the-cloud-assistant)
- [Commands for dispatched agent runs](#commands-for-dispatched-agent-runs)

## Preflight

Before using Pi Dash, verify the CLI is available and authenticated:

```bash
pidash --version
pidash workspace me
```

If `pidash` is not installed, point the user at the Pi Dash install instructions. If `workspace me` fails, ask the user to run `pidash auth login` — it is an interactive device-code flow, so do not run it yourself.

If a command or flag in this file is rejected as unknown, the installed CLI predates it. Check `--help` for what this version supports and tell the user they can upgrade with `pidash update`.

When something about the setup looks wrong, these read-only commands say what:

```bash
pidash auth status        # who this machine is logged in as, and its registered runners
pidash doctor --json      # agent binary on PATH and authenticated, git configured, cloud reachable
pidash update --check     # is a newer CLI available (changes nothing)
```

Report what they show and let the user act on it. Do not run `pidash update`, `pidash auth logout`, `pidash runner ...`, or the service commands (`install`, `start`, `stop`, `restart`, `remove`) on your own initiative — they change the machine's Pi Dash setup, not issue data.

## Output, errors, and exit codes

Every data command prints JSON on stdout, with two exceptions: `pidash ai` prints a readable transcript unless `--json` is passed, and `page get --body-only` prints plain markdown. Notes and warnings (for example a truncated-list note) go to stderr, so stdout stays parseable.

A failure prints one JSON object on stderr — `{"error": "<short message>"}`, with an optional `"detail"` carrying the server's response body — and exits non-zero:

| Exit code | Meaning                 | What to do                                               |
| --------- | ----------------------- | -------------------------------------------------------- |
| `1`       | Unknown / local failure | Read the message.                                        |
| `2`       | Invalid input           | Fix the arguments; check `--help`.                       |
| `3`       | Auth                    | Ask the user to run `pidash auth login`.                 |
| `4`       | Not found               | Re-check the identifier, state, or label name.           |
| `5`       | Server error            | Report it; do not loop.                                  |
| `6`       | Throttled               | Wait before the next call; do not retry in a tight loop. |

`detail` often names the valid values (for example the states a project actually has), so read it before guessing again.

## Project selection

Two context files exist:

- `<workspace root>/.pidash/context.md` — the one project this working directory belongs to. Authoritative for "which project am I working in". Written by `pidash context init`.
- `~/.pidash/context.md` — the projects in every workspace this machine is bound to. A lookup table for "what else exists". `pidash context refresh` updates the current workspace's entry.

When the working-directory file exists, use its project and do not ask the user. If an older file lists several projects, pick the best match from each project's `name` and `description` and report which one you used.

When it is missing:

1. Run `pidash project list`. Rows carry `id`, `identifier`, `name`, `description`, `repo_url`, `is_default`.
2. Compare `repo_url` with the checkout's remote (`git remote get-url origin`). If no project has a matching `repo_url`, compare names and descriptions with the repository name, package name, and README.
3. Ask the user to confirm the best candidate.
4. Run `pidash context init --project <identifier-or-uuid>`. It writes `.pidash/context.md` and excludes `.pidash/` from git.

When there is no human to ask, do not guess. Commands that default `--project` resolve it in this order:

1. Explicit `--project`.
2. `PIDASH_PROJECT_ID`.
3. The local default set by `pidash config set default-project <identifier-or-uuid>`.
4. The workspace default project in Pi Dash cloud.

`.pidash/context.md` is read by you, not by the CLI — pass its project as `--project`. If nothing resolves, stop and tell the user to set a default project.

Not every command defaults the project: `issue list`, `issue move`, and the `page` commands require `--project`.

## Reading issues

```bash
pidash issue get PROJ-123
pidash comment list PROJ-123
```

`issue get` prints the full issue, including `relations_summary` (`blocked_by` / `blocking`, each `{identifier, state, state_group}`) and `has_open_blockers`. Read the comments too before acting on an issue — the description is often superseded by the conversation.

## Search

Full-text search across titles, descriptions, and comments:

```bash
pidash issue search "<query>" [--project PROJ] [--status open|closed|all] [--since <ISO 8601>] [--limit <n>] [--sort rank|-created|-updated]
```

When to use:

- Before creating an issue, search the title's key phrase so you do not file a duplicate. If a strong match exists, show it to the user and ask whether to update it instead.
- When the user asks to "find", "look up", or "show issues about" something, or wants to resume work on a ticket they have not named.
- To recover how a similar problem was handled before starting related work.

Query syntax (Postgres `websearch_to_tsquery`):

- Quoted phrases for exact wording: `pidash issue search '"dark mode"'`
- `OR` for alternatives: `pidash issue search 'crash OR panic'`
- `-` to exclude: `pidash issue search 'release -alpha'`
- Stem-aware: `color` matches `colors`, `colored`.

Flags:

- `--project`: scope to one project. Omit to search the whole workspace.
- `--status`: default `all`. `open` is anything not finished; `closed` is completed or cancelled.
- `--since`: lower bound on `updated_at`, e.g. `--since 2025-01-01T00:00:00Z`.
- `--limit`: default 10, hard cap 50.
- `--sort`: default `rank` (relevance); `-created` newest first; `-updated` most recently touched first.

Never pass an empty or whitespace-only query — the server rejects it.

## Listing

Use `issue list` when you want issues by structure (state, label, parent, priority) rather than by text:

```bash
pidash issue list --project PROJ [--state-group open] [--state "In Review"] [--label bug,frontend] [--priority urgent,high] [--parent PROJ-10|none] [--fields id,sequence_id,name,state] [--order-by -created_at]
```

- `--state-group` takes `backlog`, `unstarted`, `started`, `review`, `test`, `completed`, `cancelled`, and on recent CLIs the shortcuts `open`, `closed`, `all`. Recent CLIs default to open issues when neither `--state` nor `--state-group` is given, so ask for `closed` or `all` explicitly when you need finished work.
- `--parent none` returns top-level issues only; `--parent PROJ-10` returns that issue's sub-issues.
- `--fields` trims each item. Use it — full issue payloads are large.
- `--per-page <n>` sets the page size (the server default, 1000, is also the maximum). Ask for a small page when you only need the first few.
- The result is one page. `next_page_results: true` means the list is truncated at `count` of `total_count`; pass `next_cursor` back as `--cursor` for the next page. Do not report a count as complete without checking this.

## Creating issues

```bash
pidash issue create --project PROJ --title "<title>" --description-file <path>
```

Useful flags:

- `--description-file <path>` (or `-` for stdin) for anything longer than one line; `--description "<markdown>"` for short text.
- `--priority none|low|medium|high|urgent`
- `--state "<state name>"` to start somewhere other than the project default.
- `--parent PROJ-10` to create a sub-issue.
- `--label bug,frontend` — every label must already exist in the project.
- `--pod "<name>"` / `--runner "<name>"` to route the issue to a pod or dedicate it to one runner. Leave both off unless the user asks; see [Runners, pods, and agent runs](#runners-pods-and-agent-runs).

### Writing the title

The title is what humans scan: list and board rows, notifications, search results, and the PR titles built from it. Write it as the TL;DR for a reader who has not seen your session or the code.

- Say what is wrong or wanted, and for whom — the effect, not the mechanism.
- Plain words. Keep it to about 80 characters.
- Leave out file paths, function and type names, flags, error codes, and parenthesized internals. They belong in the description.
- Start a bug title with what breaks, a feature or task title with what someone will be able to do.
- A short bracketed tag the project already uses (for example `[security]`) is fine; keep the rest readable.

| Too technical                                                                                       | Readable                                                                              |
| --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| Windows: squatting the daemon's named pipe (predictable name, first-instance create) keeps Pi Dash down until logon | Pi Dash can stay down until next login on Windows if another app takes its pipe name |
| `source` defaults to `builtin` for user-created schedulers in list serializer                        | Schedulers you create show up labeled "Built-in"                                      |
| Deep-link handler logs full `pidash://auth/callback` URL incl. code and state                       | Sign-in codes are written to the desktop app's logs                                   |

Put the precise detail in the description. A good opening line is a one-sentence technical summary (component, mechanism, file), followed by reproduction steps, evidence, and the proposed fix. Agents and search read the description; the title does not need to carry it.

When you rewrite a title with `issue patch --title`, follow the same rules.

### After creating

After creating, tell the user which project was used:

```text
Created PROJ-123 in project <project name>.
```

If the user says the issue belongs elsewhere, move it instead of recreating it:

```bash
pidash issue move PROJ-123 --project <target-identifier-or-uuid>
```

The issue may get a new identifier in the target project — report the identifier from the command's output, not the old one.

## Updating issues

```bash
pidash issue patch PROJ-123 [--state "<name>"] [--title "..."] [--description-file <path>] [--priority high] [--parent PROJ-10 | --clear-parent]
```

Pass only what changes. Before setting a state, list what the project actually has — state names are per-project:

```bash
pidash state list PROJ-123
```

`state list` takes an issue identifier or a project UUID (not a project identifier like `PROJ`).

`--description` and `--description-file` replace the whole description. To amend one, `issue get` it first, edit, and write the full text back. `--description ""` clears it.

`--work-type <key>` (for example `software` or `general`) selects which guidance Pi Dash's own agents get when they work the issue; `--clear-work-type` returns it to the project default. The server validates the key, and refuses an agent's change while the issue is being worked.

## Labels

Attach and detach on the issue:

```bash
pidash issue patch PROJ-123 --add-label bug            # keep the rest
pidash issue patch PROJ-123 --remove-label wontfix     # keep the rest
pidash issue patch PROJ-123 --label bug,frontend       # replace: anything not listed is detached
pidash issue patch PROJ-123 --clear-labels
```

Manage the project's label set:

```bash
pidash label list --project PROJ
pidash label create --project PROJ --name "<name>" [--color "#ff5630"] [--description "..."]
pidash label update "<name>" --project PROJ [--name "<new>"] [--description "..."]
pidash label delete "<name>" --project PROJ
```

Read `label list` first and reuse an existing label rather than coining a near-duplicate. Prefer `label update` to fix a misspelled label: `label delete` detaches it from every issue and page with no undo, so confirm with the user first.

## Relations and blockers

```bash
pidash issue relate PROJ-7 --blocked-by PROJ-3,PROJ-4
pidash issue relate PROJ-7 --relates-to PROJ-9
pidash issue unrelate PROJ-7 --blocked-by PROJ-3
pidash issue relations PROJ-7
```

- Pass exactly one relation flag per call: `--blocked-by`, `--blocking`, `--relates-to`, `--duplicate`, `--start-before`, `--start-after`, `--finish-before`, `--finish-after`, `--implemented-by`, `--implements`. Each takes a comma-separated list.
- `relate` is idempotent. Pairs that already carry the relation come back under `unchanged`. Pairs that carry a _different_ relation come back under `conflicts` and are left alone — `unrelate` the old one first.
- `unrelate` removes only that exact relation; pairs without it come back under `not_related`, which is not an error.

Use `--blocked-by` when splitting work into issues that must land in order, so Pi Dash and later agents can see the dependency.

## Comments

```bash
pidash comment list PROJ-123
pidash comment add PROJ-123 --body-file <path> --as-agent "<runtime name>" [--agent-run-id "$PIDASH_RUN_ID"]
pidash comment update PROJ-123 <comment-uuid> --body-file <path>
```

- When the comment is your own message, always pass `--as-agent` with your runtime name — `Claude Code`, `Codex`, or `AI Agent` when unknown. The authenticated CLI user remains the audit actor; `--as-agent` is what lets Pi Dash and later agent prompts tell agent messages from human replies.
- Add `--agent-run-id "$PIDASH_RUN_ID"` when that variable is set — Pi Dash sets it on agents it dispatches. Outside a Pi Dash agent run it is unset: omit the flag, but keep `--as-agent`. `--agent-run-id` without `--as-agent` is rejected.
- `--body-file` takes a real path here; `-` for stdin is not supported on comments.
- When relaying the user's own words as a comment on their behalf, omit `--as-agent`.
- `--image <path>` uploads and embeds an image; repeat it for several.
- `--fold` marks a low-value status comment so the UI collapses it and future agent prompts skip it. Use it for progress chatter, never for a question or a result the human needs.
- `comment update` edits a comment you own and replaces its whole body. An empty body is rejected unless you pass `--allow-empty`, so an accidentally empty file cannot wipe a comment.
- `--body "<text>"` is fine for a one-line comment; use `--body-file` for anything longer.

Write comments for a human who has not seen your session: what changed, what you need from them, and links.

## Pull requests and merge requests

When you open a GitHub pull request or GitLab merge request for an issue, do both, in this order:

1. **Comment the link on the issue** (see [Comments](#comments)). Comments are the main bridge to the human; this is how a reviewer follows the work.
2. **Attach it**, so Pi Dash records a structured link and shows live status (open / draft / merged / closed) on the issue:

   ```bash
   pidash issue attach-review PROJ-123 --url <pull-or-merge-request-url>
   ```

Notes:

- The comment is for the human; the attachment is the machine-readable association. One does not replace the other.
- `attach-review` is idempotent: re-running it for the same URL returns the existing link.
- One issue may have several attached reviews; a given review attaches to exactly one issue. Attaching one that is already linked to a different issue is rejected.
- `pidash issue attach-pr` is the older name for the same command and still works. Use it if `attach-review` is not available on the installed CLI.

## Pages

Pages are the project's wiki — design notes, runbooks, conventions.

```bash
pidash page list --project PROJ [--include-archived]
pidash page get <page-uuid> --project PROJ --body-only > page.md
pidash page create --project PROJ --title "<title>" --body-file <path> [--parent <page-uuid>] [--access public|private]
pidash page update <page-uuid> --project PROJ [--title "..."] [--body-file <path>]
pidash page archive <page-uuid> --project PROJ
```

- `page list` returns metadata only; `page get` returns the body. `--body-only` prints plain markdown instead of the JSON envelope.
- Check `page list` for project conventions before starting unfamiliar work in a project.
- `page update --body-file` replaces the whole body. To amend a page, `page get --body-only` it, edit, and write it back. An empty body is rejected unless you pass `--allow-empty`.
- `page list` is paginated like `issue list`; watch for `next_page_results`.
- Archiving is reversible with `page unarchive`.

## Runners, pods, and agent runs

Pi Dash can dispatch its own coding agents on an issue. From a session you can inspect and steer that:

```bash
pidash project runners [--project PROJ] [--assignable-for PROJ-123|new]
pidash issue patch PROJ-123 --runner "<name>"      # dedicate the issue to one runner
pidash issue patch PROJ-123 --clear-runner         # back to any runner in the pod
pidash issue patch PROJ-123 --pod "<name>"
pidash issue run-ai PROJ-123                        # same as clicking "Run AI" in the web app
pidash issue re-tick PROJ-123                       # re-grant an exhausted ticking budget
```

- `run-ai` starts a real agent run and spends budget. Do it only when the user asks. It exits non-zero when nothing could be dispatched; the error `detail` carries a machine-readable `reason` (`active_run_exists`, `no_pod`, `no_eligible_runner`, or an `assigned_runner_*` refusal) and, for dedicated-runner refusals, the `options` to offer the user next.
- When `run-ai` is refused because the issue's dedicated runner is offline or paused, `pidash issue run-ai PROJ-123 --pod-once` runs this one run on any runner in the pod and leaves the issue dedicated. Offer it only when the work does not need that specific machine; otherwise report that the runner is unavailable and stop.
- Check `pidash project runners --assignable-for PROJ-123` before choosing a runner, and do not pick one whose row says it is not assignable. A dedicated runner is a bottleneck by design — leave an issue on its pod unless it needs something only one runner has.
- `re-tick` is a no-op unless the issue is in a ticking state and its budget is used up; check `granted` in the output.
- These commands are newer than the rest — confirm with `--help` that the installed CLI has them.

## Schedulers

Schedulers run a prompt on a project on a cadence.

```bash
pidash scheduler list [--project PROJ]
pidash scheduler get <scheduler-id> [--project PROJ]
pidash scheduler runs <scheduler-id> [--project PROJ] [--limit <n>]
```

`<scheduler-id>` is a UUID or the scheduler's slug.

Reading is safe. Writing is not casual: `scheduler install`, `run-now`, `uninstall`, and `update` of an install (cadence, enabled, pod, extra context) need project-admin rights and change what runs on the project for everyone; `scheduler create`, `edit`, `delete`, and `update` with `--prompt-file`, `--name`, or `--description` need workspace-admin rights and change a definition every installing project shares. Do these only when the user asked for that specific change, read `--help` for the exact flags, and show the user the `previous` values the command prints.

The write commands, once the user has asked for the change:

```bash
# Put a scheduler on a project. --rrule is an RFC 5545 rule; "" means one shot at --dtstart.
pidash scheduler install <scheduler-id> --project PROJ --rrule "FREQ=DAILY;BYHOUR=2" [--tzid America/New_York] [--dtstart <ISO 8601>] [--outcome-mode create_issue|apply_fix|fix_and_review] [--extra-context-file <path>] [--disabled]

# Change one install: cadence, on/off, pod, or the project-specific extra context.
pidash scheduler update <scheduler-id> --project PROJ [--rrule "..."] [--enable | --disable] [--extra-context-file <path>] [--if-updated-at <updated_at>]

pidash scheduler run-now <scheduler-id> --project PROJ
pidash scheduler uninstall <scheduler-id> --project PROJ

# Workspace-wide definitions. The prompt is never taken inline: use --prompt-file (or --prompt - for stdin).
pidash scheduler create --name "<name>" --slug <slug> --prompt-file <path>
pidash scheduler edit <scheduler-id> [--prompt-file <path>] [--name "..."] [--enable | --disable] [--if-updated-at <updated_at>]
pidash scheduler delete <scheduler-id>
```

- Pass `--if-updated-at` with the `updated_at` you read from `scheduler get`, so a concurrent edit is refused (412) instead of overwritten.
- `uninstall` and `delete` ask for confirmation on a terminal. `--yes` skips the prompt — pass it only after the user has confirmed in this conversation.
- `run-now` fires within about a minute, and fails with a 409 when the install is disabled or one of its runs is still active.
- A new definition runs nowhere until it is installed on a project. Built-in schedulers can be installed but not edited or deleted.

## Asking the cloud assistant

```bash
pidash ai "<question or instruction>" [--json] [--timeout <seconds>]
```

Sends one message to the workspace's Pi Dash cloud assistant and prints its reply. It needs an LLM provider configured in Pi Dash. Prefer the direct commands above when you know what you want — they are faster and deterministic. Do not use `pidash ai` unless the user asks for it.

## Commands for dispatched agent runs

These exist for an agent that Pi Dash itself started (the daemon sets `PIDASH_RUN_ID` and `PIDASH_ISSUE_IDENTIFIER`). That agent's prompt says when to call them. In an ordinary interactive session, leave them alone:

- `pidash workpad get` / `pidash workpad update --body-file <path>` — the agent's per-issue scratch notes. `update` overwrites the whole workpad.
- `pidash run yield --outcome <outcome>` — reports the run's outcome; called once, last.
- `pidash issue wait <identifier>` — records a decision to wait on an open blocker.
- `pidash issue patch <identifier> --git-work-branch <branch>` — records the branch later runs should check out.
