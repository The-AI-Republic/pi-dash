# `pidash` CLI reference

On-demand reference for the commands the prompt section `Pi Dash CLI`
summarises. Plain markdown on stdout, so `pidash docs run` prints one topic. Every `pidash …` command line in this file parses with the installed
binary's clap `Command` (a contract test holds that); angle brackets
mark values you supply, square brackets mark flags you may omit.

`pidash` is already authenticated **as this runner's owner**; your writes
carry their identity plus this run's id. Never pass credentials as flags,
and never print, log or record any `PIDASH_*` value or anything from the
pidash config. Inside an agent run `PIDASH_ISSUE_IDENTIFIER`,
`PIDASH_PROJECT`, `PIDASH_RUN_ID` and `PIDASH_AGENT_NAME` are set for you,
so commands default to this issue and run.

Every command prints one JSON document to stdout and exits 0 on success.
On failure it prints `{"error": …}` on stderr and exits non-zero: read it,
retry only transient failures, and never retry the same command more than
twice.

Commands that manage the runner itself (`login`, `auth`, `config`,
`runner`, `install`, `update`, `engine`, `reconnect`, `ai` and the rest)
are refused inside an agent run with
`{"error": "operator command not available inside an agent run"}`; if one
seems necessary, the run is blocked. `pidash --help` outside a run lists
them all. `pidash issue re-tick <identifier>` is a human decision; never call it.

All writes are real and immediate; there is no undo. When pasting `pidash`
JSON into a comment for audit, enclose it in a fenced
code block.

## Issue

Work items: fetch, list, create, change state, edit fields, relate and
wait. The identify of an issue is its project-scoped identifier such as
`ENG-42` (or its UUID); the server resolves names to UUIDs for you.

- `pidash issue get <identifier>` — fetch a work item. Returns the full
  payload including `id`, `name` (title), `description`, `state` (UUID —
  pair with `pidash state list` to map back to a name), `priority`,
  `labels`, `assignees`, timestamps, and a blocker summary:
  `relations_summary` (`blocked_by` / `blocking` lists of `{identifier,
state, state_group}`) plus `has_open_blockers` (true while any
  `blocked_by` item is not in a `completed` / `cancelled` group).
  Single-item reads also carry `relations`: every relation grouped by
  type, each `{id, identifier, name, state, state_group}` (see
  `pidash issue relations <identifier>`.
  Single-item reads also carry `active_run` while a run occupies the
  issue's run slot: `{id, status, runner_id, created_at, assigned_at,
  started_at, age_seconds, runner_last_heartbeat_at, branch}` (`branch`
  is the run's working branch, `null` until one is known); the key is
  absent when no run is active.
- `pidash issue busy <identifier>` — check whether an agent run owns the
  issue's branch **before you push to it**. Prints `{issue, busy,
  owned_by_caller, branch, active_run, advice}`. `busy` is `true` while
  another agent run is active on the issue (from `queued` on);
  `owned_by_caller` is `true` when that run is your own (`PIDASH_RUN_ID`),
  and then `busy` is `false`. `branch` is `active_run.branch`, else the
  issue's `git_work_branch`, else `null`. Read-only: it takes no lock, and
  exits 0 whenever the read succeeds — check `busy`. **Convention:** an
  agent working outside the issue's run (a coordinator, a reviewer, a
  person's script) that finds `busy: true` must not push to `branch`;
  it comments on the issue with the finding or fix instead, and the
  owning run picks it up. Check again just before the push, not only at
  the start of the work.
- `pidash issue list --project <PROJ-or-UUID> [--cursor <c>] [--per-page <N>] [--order-by <field>]` — list work items in a project.
  `--project` accepts either the workspace-scoped slug (e.g. `ENG`) or a
  project UUID. Returns the server's paginated envelope `{count,
total_count, next_cursor, prev_cursor, next_page_results, results:
[...]}`. **One call returns at most one page — 1000 items, which is
  also the server maximum — so always check for truncation:** when
  `next_page_results` is `true`, only `count` of `total_count` items were
  returned; pass `next_cursor` back via `--cursor` to fetch the rest (the
  CLI also prints a stderr note when this happens). Use this to find
  related/duplicate issues in the same project before creating new ones.
  - Filters (each comma-separated; values OR within a flag, flags AND
    together): `--state <name-or-UUID>` (unknown names fail and list the
    valid ones), `--state-group <group>`
    (plus `open` (`backlog,unstarted,started,review,test`), `closed`
    (`completed,cancelled`), `all` (no filter) — the full choice list is
    `backlog|unstarted|started|review|test|completed|cancelled|open|closed|all`;
    with neither `--state` nor `--state-group` the list defaults to `open`;
    the response carries
    "state_group": "open" (or the alias you passed)), `--parent
<PROJ-123|UUID|none>` (`none` = top-level only), `--label
<name-or-UUID>`, `--priority <urgent|high|medium|low|none>`. Add
    `--fields id,sequence_id,name,state,parent` for a small payload.
    Example: `pidash issue list --project ENG --parent ENG-12 --state Backlog --fields id,sequence_id,name,state`
    lists the Backlog sub-issues of `ENG-12` in one call — prefer this over
    paging the whole project and calling `pidash issue get <identifier>` on each item.
    `--parent` is also how a later run finds the children an earlier run
    already created, so a split never files duplicates.
- `pidash issue create --project <PROJ-or-UUID> --title <title> [--description <md>] [--description-file <path>] [--priority <priority>] [--state <state-name-or-UUID>] [--parent <PROJ-123-or-UUID>] [--label <name-or-UUID>]` —
  file a new work item under the named project. Use this for capturing
  **discovered scope that does not belong on the current issue** (a
  follow-up bug, a separated task, a missing prerequisite), or — with
  `--parent` — for filing a **child issue** when you split an issue into
  genuinely separate tasks. Do **not** create an issue to track sub-steps
  of the current run. `--parent` attaches the
  new item as a sub-issue of the given issue (project-scoped identifier
  like `PROJ-123`, or a raw UUID); the server validates the parent.
  `--label` attaches labels that **already exist** in the project. Mention
  the new issue's identifier in your comment so a person can find it.
  The description is markdown and keeps its structure (headings, nested
  and task lists, code blocks, tables). For a long or multi-line body,
  write it to a file or pipe it with `--description-file -` instead of
  quoting it inline, since shell quoting of long markdown is error-prone.
  Empty input is an error.
- `pidash issue patch <identifier> --state <state-name>` — move another
  issue to a different state. The name is case-insensitive; the CLI
  resolves it to a UUID for you. You can also pass a state UUID directly.
  A run moves its own issue with the `run finish` command, not this: the
  server refuses a run moving its own issue to Done, Cancelled or a stage
  the project switched off.
- `pidash issue patch <identifier> [--title <s>] [--description <md>] [--description-file <path>] [--priority <priority>] [--parent <PROJ-123-or-UUID>] [--clear-parent] [--add-label <name-or-UUID>] [--remove-label <name-or-UUID>] [--label <name-or-UUID>] [--clear-labels] [--work-type <key> --clear-work-type] [--git-work-branch <branch>]` —
  update other fields. At least one flag is required. `--parent`
  re-parents the issue under the given issue (identifier or UUID), e.g. to
  attach an existing issue to a tracking parent; `--clear-parent`
  detaches it to top-level. `--parent` and `--clear-parent` are mutually
  exclusive. `--work-type` sets the issue's work type (e.g. `software`),
  which selects the per-work-type guidance the agent prompts carry;
  `--clear-work-type` removes it so the issue inherits the project default,
  and the two are mutually exclusive. `--git-work-branch` records the
  branch the issue's work lives on, so later runs check it out instead of
  cutting a new one — set it only after the branch exists on the remote.
  Prefer `--add-label` / `--remove-label` over `--label`: they
  keep the labels a human already set, while `--label` and
  `--clear-labels` replace the whole set. Do **not** edit title or
  description for planning or progress tracking.
  The one case where `--description` is correct is recording the child
  list on a **tracking parent** after a split.
- `pidash issue move <identifier> --project <PROJ-or-UUID>` — move a work
  item into another project in the same workspace.
- `pidash issue attach-review <identifier> --url <url>` — attach a GitHub
  pull request or GitLab merge request to a work item so Pi Dash links it
  and tracks its status. Idempotent: one issue may have many code reviews,
  but one code review attaches to exactly one issue.
  `pidash issue attach-pr <identifier> --url <url>` is the same command
  under its script-friendly alias.
- `pidash issue relate <identifier> --blocked-by <ID>` — record that the
  issue cannot finish until each listed issue does. Other flags (exactly
  one per call): `--blocking`, `--relates-to`, `--duplicate`,
  `--start-before`, `--start-after`, `--finish-before`,
  `--finish-after`, `--implemented-by`, `--implements`; each names the
  relation from the issue's side, so relating `B` with `--blocked-by A`
  and relating `A` with `--blocking B` are the same edge. IDs are
  identifiers or UUIDs. Idempotent: already-related pairs come back under
  `unchanged`, and a pair that already has a _different_ relation comes
  back under `conflicts` untouched (unrelate it first if you mean to
  change it). Prints `{issue, relation_type, created, unchanged,
conflicts, relations}`.
- `pidash issue unrelate <identifier> --blocked-by <ID>` — remove that
  exact relation (same flags as relate). Pairs without it come back under
  `not_related`; that is not an error.
- `pidash issue wait <identifier>` — record that you have read this
  issue's open blockers, decided you cannot safely proceed, and are
  yielding. It buys back the tick this run is spending, so waiting costs
  no net budget: call it, then finish the run with `--result waiting_on_external`. Note what you are waiting on in the workpad's Keep in mind first — the
  platform does not read it, but the next run will. Read `applied`
  and `reason` in the printed `{applied, reason, used, granted, waited,
cap}`: a refusal (`wait_cap_reached` once the one-pool allowance is
  spent, `infinite_pool` when the issue has no cap, `no_ticker`) is a
  normal exit 0, changes nothing, and means further waiting spends normal
  budget — tell the human in a comment rather than looping on it. A
  transport or permission error exits non-zero like any other CLI call;
  yield anyway rather than retrying. Calling it twice in one run buys two
  ticks while the allowance lasts.
- `pidash issue relations <identifier>` — list the issue's relations
  grouped by type from its side (`blocked_by`, `blocking`,
  `relates_to`, …), each `{id, identifier, name, state, state_group}`.
  `pidash issue get <identifier>` carries the same block as `relations`.
- `pidash issue search <query> [--project <PROJ-or-UUID>] [--status <status>] [--state-group <group>] [--since <iso8601>] [--limit <N>] [--sort <order>]` —
  full-text search across work item titles, descriptions, **and** comments.
  Use this to recover historical context: check whether a similar issue has
  been filed before, look up the _resolution_ of a past task (the answer
  usually lives in a comment, not the title), or find anything that touched
  a given component. **This is search, not grep**: the query is parsed as
  websearch syntax (quoted phrases, `OR`, `-exclude`) and is stem-aware;
  regex / glob will not work. `--since` accepts ISO 8601 — either a full
  datetime (`2025-01-01T00:00:00Z`) or a bare date (`2025-01-01`, treated as
  midnight that day); malformed strings produce a 400. `--sort` accepts
  only `rank` (default), `-created`, `-updated`; other values 400. Returns
  `{query, count, results: [{id, identifier, name, snippet, state, project,
created_at, updated_at, completed_at, rank, url}]}`. `snippet` is empty
  when the match was in the title or in a comment (the snippet excerpt only
  covers the description body). Default `--limit` is 10, server max is 50 —
  tuned for your context window, not bulk export. Cast a broad net first,
  then read promising hits with `pidash issue get <identifier>`.

## Comment

Comments are the human ↔ agent conversation channel. Use them to ask
clarifying questions, post blocker notices, share links to deliverables,
and announce completion. **Notes for the next agent run go in the
workpad, not in comments.**

- `pidash comment list [<issue>] [--since <time>]` — list comments on the
  issue, oldest first. The issue defaults to `PIDASH_ISSUE_IDENTIFIER`.
  Each entry has `id` (UUID), `comment_html`, `comment_stripped`,
  `labels`, `actor_detail`, `speaker_type`, `speaker_label`,
  `speaker_agent_run_id`, and timestamps. With `--since` (RFC 3339, e.g.
  `2026-11-20T09:34:00Z`) only comments created after that time print, in
  the same JSON shape — and an empty list when there are none. Use
  `--since` to check for human replies newer than the prompt instead of
  re-reading the whole thread.
- `pidash comment add [<issue>] --body-file <path> [--image <path>] [--fold]` — post a new
  comment from a file and mark it as spoken by this AI agent run.
  `--body <markdown>` works for one-liners. Prefer `--body-file` for
  anything multi-line — shell quoting of markdown is error-prone.
  `--image <path>` uploads an image and embeds it in the comment;
  repeatable for several images, and at least one image or a body is
  required. The
  speaker defaults come from the run: `--as-agent` from
  `PIDASH_AGENT_NAME`, `--agent-run-id` from `PIDASH_RUN_ID` — so `pidash comment add --body-file <path>` with no other flags posts as the agent run, and
  explicit flags win. Flag-level detail: `--as-agent <name>` marks the
  comment spoken by an AI agent with that display name; `--agent-run-id
<uuid>` attaches the producing run (it requires `--as-agent`). Outside
  a run both stay unset and the comment is attributed to the human owner,
  as posting in your own workspace does.
- `pidash comment update <identifier> <comment-id> --body-file <path> [--allow-empty]` —
  edit a comment you own. Both the issue identifier and the comment UUID
  are required. Rarely needed — prefer posting a fresh comment for new
  information. An empty body is rejected unless `--allow-empty` is passed.

Use `--fold` only for low-value status/noop updates that should remain
available to humans without cluttering the thread. Folded comments are
collapsed by default in the UI and omitted from future agent-run task
prompts, though `pidash comment list` still returns them. Pi Dash never
folds comments automatically. Do not fold questions, blockers, decisions,
results, or other context a future run needs.

## Workpad

The workpad is the shared memory of the agent runs on one issue — In
Progress, review and test alike: a short markdown note of what happened
and what is true (`### Keep in mind` bullets and a `### Log` line per
run). It is memory, not instructions for the next run, and it is **not**
visible to humans in the comment thread.

Before the agent launches, the runner writes the run's workpad snapshot
to `.pidash/workpad.md` (empty when the workpad is empty) with its guard
hash in `.pidash/workpad.sha256`. Edit that file, then run
`pidash workpad update`; there is no append, so the file is always the
full body. If the file is missing, run `pidash workpad get --to-file`
first.

- `pidash workpad get [<identifier>]` — fetch the current workpad body.
  Returns `{body, updated_at, body_sha256}`. Defaults the identifier to
  `PIDASH_ISSUE_IDENTIFIER` so you can call it bare.
- `pidash workpad get [<identifier>] --to-file [<path>]` — write the body
  to `<path>` (default `.pidash/workpad.md`) and the hash to its
  `workpad.sha256` sibling, and print `{updated_at, body_sha256, path}`
  instead of the body, so a refresh does not load the whole workpad into
  your context.
- `pidash workpad update [--body-file <path>] [--allow-empty]` — overwrite
  the workpad body from a file (`--body-file` defaults to
  `.pidash/workpad.md`). An empty file is refused unless `--allow-empty`
  is passed, so a failed get (e.g. 429) that produced an empty file cannot
  silently wipe the stored workpad; pass `--allow-empty` only for an
  intentional clear. The command sends the `workpad.sha256` sibling as the
  `if_body_sha256` guard and rewrites it from each response, so two
  consecutive updates succeed without an intervening get. On a 412 the
  command exits non-zero telling you to run `pidash workpad get --to-file`
  and re-apply your edit: somebody (or your parallel self) changed the
  workpad since you read it. The body file is kept by default (`--keep`
  is a no-op; `--remove` deletes it after a successful upload).

## Page

Project pages are the project's wiki: durable shared knowledge —
decisions, conventions, plans — that humans and future runs read. A
workpad is different: the agent runs' short note for one issue. Read pages
**before** you re-derive a convention from the code — a page is cheaper
and more authoritative than a guess. Write one only when you have
knowledge worth keeping for the project, not progress notes.

- `pidash page list --project <PROJ-or-UUID> [--cursor <c>] [--per-page <N>] [--include-archived]` —
  list the project's pages. Returns the usual paginated envelope
  (`{count, total_count, next_cursor, prev_cursor, next_page_results,
results: [...]}`; while `next_page_results` is `true`, pass
  `next_cursor` back via `--cursor` for the rest) with **metadata only** —
  `id`, `name`, `parent`, `owned_by`, `access`, `is_locked`,
  `archived_at`, timestamps. Archived pages are omitted unless you pass
  `--include-archived`. A page another member marked private is not
  listed.
- `pidash page get <page-id> --project <PROJ-or-UUID>` — fetch one page by
  its UUID (take it from the list output). Prints the JSON envelope: the
  metadata above plus `description_html`, `description_stripped`, and
  `description_markdown`. Prefer `description_markdown`.
- `pidash page get <page-id> --project <PROJ-or-UUID> --body-only` — print
  just the markdown body, so you can redirect it to a file.
- `pidash page create --project <PROJ-or-UUID> --title <title> [--body <md>] [--body-file <path>] [--parent <page-id>] [--access <access>]` —
  create a page; the body is markdown. `--body-file -` reads stdin.
  **List first** and prefer updating an existing page over creating a
  near-duplicate.
- `pidash page update <page-id> --project <PROJ-or-UUID> [--title <s>] [--body <md>] [--body-file <path>] [--parent <page-id>] [--clear-parent] [--access <access>] [--allow-empty]` —
  change only the fields you pass (at least one). A body flag replaces
  the whole body — fetch with `--body-only` first, edit, then write it
  back. An empty body is rejected unless `--allow-empty` is passed.
  Exit 2 with a 409 means the page is locked or archived; exit 5
  with a 503 means the live document service is down — retry later, do
  not work around it.
- `pidash page archive <page-id> --project <PROJ-or-UUID>` — archive a
  page (and `pidash page unarchive <page-id> --project <PROJ-or-UUID>` to
  restore it). Do **not** archive a page you did not create unless the
  issue asks you to.

## Label

Labels are the project's free-form tags — `bug`, `frontend`,
`needs-design`. They are the dimension states and parents don't cover: a
state says _where_ the work is, a label says _what kind_ it is or _what
it touches_. Humans filter and group boards by them, so a wrong label is
visible immediately.

Read before you write. `pidash label list` is one call and tells you the
vocabulary this project already uses; reach for `pidash label create --name <name>`
only when nothing existing fits, because a near-duplicate (`front-end`
next to `frontend`) is worse than no label at all.

- `pidash label list [--project <PROJ-or-UUID>] [--cursor <c>] [--per-page <N>]` — list the project's
  labels with `id`, `name`, `color`, `description`, and `parent`. Returns
  the usual paginated envelope. Everywhere else a label is named you may
  pass its name (case-insensitive) or its UUID; names are unique within a
  project.
- `pidash label create --name <name> [--project <PROJ-or-UUID>] [--color <color>] [--description <s>] [--parent <name-or-UUID>]` —
  create a label. A name that already exists is a 409, not a silent reuse.
  Fill in `--description` when the name alone won't tell the next run
  what the label means. `--parent` nests it under an existing label;
  groups are cosmetic, for keeping a long settings list readable.
- `pidash label update <name-or-UUID> [--project <PROJ-or-UUID>] [--name <s>] [--color <color>] [--description <s>] [--parent <name-or-UUID>] [--clear-parent]` —
  change a label in place. At least one flag is required. The edit lands
  on **every** work item already carrying it, so renaming is the right
  fix for a typo'd label and re-labelling each issue is not.
- `pidash label delete <name-or-UUID> [--project <PROJ-or-UUID>]` — delete
  a label and detach it from every work item and page that carries it.
  There is no undo and the deletion is invisible in each affected issue's
  history. Do **not** delete a label unless the issue explicitly asks you
  to; rename it instead.

Attach and detach labels through the issue commands:

```sh
pidash issue list --project ENG --label bug
pidash issue create --project ENG --title <title> --label bug,frontend
pidash issue patch ENG-42 --add-label needs-design
pidash issue patch ENG-42 --remove-label needs-design
```

`--add-label` and `--remove-label` are relative and repeatable, and each
accepts a comma-separated list; they read the issue's current labels and
write back the merged set, so the labels a human already applied survive.
`--label` and `--clear-labels` replace the whole set — reach for them only
when you genuinely mean "these and nothing else". A label named in both
`--add-label` and `--remove-label` is rejected rather than resolved. Every
label must already exist in the issue's project: an unknown name exits
non-zero and the error's `detail` lists the labels that do exist, so
correct from that list rather than guessing.

## State

- `pidash state list [--project <PROJ-or-UUID>] [<issue>]` — list the
  workflow states with `name`, `group` (`backlog | unstarted | started |
review | test | completed | cancelled`), and `description`. Defaults to
  `PIDASH_ISSUE_IDENTIFIER`, then `PIDASH_PROJECT`; pass an issue
  identifier or a project UUID-or-identifier to override. Already shown
  in the prompt under "Available states"; only call again if something
  looks stale.

## Scheduler

Schedulers are the workspace's recurring agent jobs — a prompt plus a
cadence, installed onto a project, firing agent runs on a timer. Use the
read commands to _audit_ scheduled automation: what is installed, whether
it is enabled, when it fires next, and what its recent runs actually did.
The writes need admin rights. Changes to a project's **install** of a
scheduler need project admin; changes to the scheduler **definition**,
which every project installing it shares, need workspace admin. A 403
means the credential you run as lacks the role: say so, do not work
around it. Use the writes only when the task asks you to change scheduled
automation; never to quiet, re-fire, remove or add a scheduler on your
own initiative. Built-in schedulers (`is_builtin: true`) can be installed
and uninstalled, but their definitions cannot be edited or deleted. Every
command takes the scheduler's UUID or its slug.

- `pidash scheduler list [--project <PROJ-or-UUID>]` — with `--project`,
  the schedulers installed on that project, each with its install's
  cadence (`dtstart`/`rrule`/`tzid`), `enabled`, `next_run_at`, and
  last-run summary (`last_run`, `last_run_status`, `last_error`); without,
  every scheduler definition in the workspace (bindings shown only for
  projects you can access). Returns the usual paginated envelope.
- `pidash scheduler get <scheduler> [--project <PROJ-or-UUID>]` — one
  scheduler's full definition: `prompt`, `description`, `is_builtin`,
  `is_enabled`, and its install on the project (cadence, `outcome_mode` —
  what its runs do with findings, `pod`, `extra_context`, `last_error`).
  404 when the scheduler is not installed on that project.
- `pidash scheduler runs <scheduler> [--limit <N>] [--project <PROJ-or-UUID>]` —
  recent agent runs the scheduler fired on that project, newest first:
  `status`, `trigger`, `created_at`/`started_at`/`ended_at`,
  `error_code`, and `issues` — the work items the run wrote comments to,
  so you can trace what a scheduled run touched. Default 30, max 100.
- `pidash scheduler update <scheduler> [--project <PROJ-or-UUID>] [--extra-context-file <path>] [--extra-context <text>] [--enable] [--disable] [--rrule <rule>] [--dtstart <iso8601>] [--tzid <tz>] [--rdate <rdate> --clear-rdates] [--exdate <exdate> --clear-exdates] [--outcome-mode <mode>] [--pod <uuid> --clear-pod] [--if-updated-at <updated_at>]` —
  edit the install; only the flags you pass change. `extra_context` is
  multi-paragraph markdown and **replaces the whole text**, so read it
  first with `pidash scheduler get <scheduler>`, edit a file, and send it with
  `--extra-context-file <path>` (`-` reads stdin) rather than quoting it
  inline. A cadence flag recomputes `next_run_at`. Repeatable `--rdate`
  adds an extra one-off firing and `--exdate` a firing to skip (ISO 8601
  datetimes); each replaces the whole list, while `--clear-rdates` and
  `--clear-exdates` empty it — each clear is mutually exclusive with its
  set flag. `--clear-pod` drops the pod override so the
  install uses the project default pod, and is mutually exclusive with
  `--pod`. Pass `--if-updated-at`
  with the install's `updated_at` from your read to fail with a 412
  instead of overwriting an edit made in between. Prints the updated
  install plus `previous` — the prior value of every field you changed;
  keep it, it is how a bad edit is reverted.
- `pidash scheduler update <scheduler> [--prompt-file <path>] [--name <s>] [--description <s>] [--if-updated-at <updated_at>]` —
  edit the definition instead; no `--project`, and these flags cannot be
  mixed with the install flags above (run two commands). The prompt
  **replaces the whole text** and is only ever read from a file: read it
  with `pidash scheduler get <scheduler>`, edit a file, send it back. Prints the
  definition, `previous` (keep it — it is the revert), and
  `affected_projects`: every project whose install now runs the new
  prompt — check that list is what you intended. Exit 2 with a 409 and
  `code: builtin_scheduler` means the scheduler is built-in and not
  editable; put the change in an install's `extra_context` instead.
  `--if-updated-at` here takes the scheduler's own `updated_at`, not the
  install's.
- `pidash scheduler run-now <scheduler> [--project <PROJ-or-UUID>]` — make
  the install due so it fires on the scanner's next tick (within about a
  minute); its regular schedule resumes afterwards. Exit 2 with a 409
  means that tick could not produce a run — the install or the scheduler
  is disabled, a run from it is still active (queued counts), or its
  schedule has no future occurrence. Read the `code` in the error detail;
  do not retry in a loop.
- `pidash scheduler create --name <s> --slug <slug> --prompt-file <path> [--description <s>] [--color <#rrggbb>] [--disabled]` —
  create a definition (workspace admin). It runs nowhere until installed.
  The prompt is read from a file, never inline. A taken or built-in slug
  exits 2 with a `slug` field error. Prints the new definition.
- `pidash scheduler edit <scheduler> [--prompt-file <path>] [--name <s>] [--description <s>] [--color <#rrggbb>] [--enable] [--disable] [--if-updated-at <updated_at>]` —
  edit a user-created definition (workspace admin); only the flags you
  pass change. Same output and rules as `update --prompt-file`:
  `previous` is the revert, `affected_projects` the blast radius, and a
  built-in scheduler exits 2 with `code: builtin_scheduler`. `--disable`
  stops every install of it from firing.
- `pidash scheduler delete <scheduler> [--yes] [--if-updated-at <updated_at>]` —
  delete a user-created definition and uninstall it from **every** project
  (workspace admin). Run `pidash scheduler list --project <PROJ-or-UUID>` first to see where it is
  installed. Without `--yes` it asks for confirmation, which fails when
  there is no terminal — pass `--yes` only when the task asks for the
  delete. Built-in schedulers exit 2 with `code: builtin_scheduler`.
- `pidash scheduler install <scheduler> [--project <PROJ-or-UUID>] --rrule <rule> [--dtstart <iso8601>] [--tzid <tz>] [--rdate <rdate>] [--exdate <exdate>] [--outcome-mode <mode>] [--pod <uuid>] [--disabled]` —
  install a scheduler on a project (project admin). `--dtstart` defaults
  to now; `--rrule ""` is a single shot at `--dtstart`. Repeatable
  `--rdate` adds an extra one-off firing and `--exdate` a firing to skip
  (ISO 8601 datetimes). Prints the
  install with `next_run_at` set. Exit 2 with a 409 means `code:
already_installed` (use `update` instead) or `scheduler_disabled` (the
  definition is off).
- `pidash scheduler uninstall <scheduler> [--project <PROJ-or-UUID>] [--yes]` —
  remove the scheduler from that project (project admin); the definition
  and its other installs stay. Asks for confirmation unless `--yes`.

## Pods

An issue routes to a **pod** — any free runner in that squad takes the run —
or to **one dedicated runner**, chosen because of the machine, tools or
access that runner has. A dedicated runner is a bottleneck by design: every
run of the issue goes to it and to no other.

- `pidash project list` — list the projects in the active workspace, each
  with `id`, `identifier`, `name`, `description`, `repo_url` and
  `is_default`. Use it to resolve a project's slug before scoping other
  commands to it.
- `pidash project runners --project <PROJ-or-UUID> [--assignable-for <mode>]` —
  list the project's runners. With `--assignable-for`, each row carries
  `assignable`, `not_assignable_code`, `is_busy` and `queue_depth`: check it
  before choosing, and do not choose a runner that is not assignable.
- `pidash issue patch <identifier> --runner <name-or-UUID>` — dedicate the
  issue to one runner. Names resolve within the issue's project, among
  runners you can see; an ambiguous name errors and lists the matches.
  `--runner` and `--clear-runner` are mutually exclusive.
- `pidash issue patch <identifier> --clear-runner` — return the issue to
  "any runner in the pod".
- `pidash issue create --project <PROJ-or-UUID> --title <title> --pod <name-or-UUID>` —
  set the pod on creation (full `<PROJ>_<name>` or bare `<name>`); without
  `--runner` this also clears any dedicated runner, and with `--runner` the
  two must agree.
- `pidash issue get <identifier>` prints `assigned_pod`,
  `assigned_runner`, `assigned_runner_detail`, and `dispatch_wait` when a
  run is waiting for its runner.
- `pidash issue run-ai <identifier> [--pod-once]` — start a run.
  `--pod-once` runs on any runner in the pod for this run only, instead of
  the dedicated runner; the issue stays dedicated. A refusal
  (`assigned_runner_offline`, `assigned_runner_paused` with its
  `pause_reason`, `assigned_runner_not_owner`) exits non-zero and prints
  the refusal body with its `options`: retry with `--pod-once` when the work
  does not need the dedicated runner's machine, or change the runner.
- A run that reports `dispatch_wait` is **waiting, not broken**. Do not
  create another run or another issue for it. A run refused with
  `assigned_runner_offline` or `assigned_runner_paused` is not an error to
  retry in a loop: use `--pod-once` only when the work does not need the
  dedicated runner's machine; otherwise report that the runner is
  unavailable and stop.
- There is no release for a dedicated run. To move the issue, use `--runner
<other>` or `--clear-runner`.
- **Leave an issue on its pod unless the work needs something only one
  runner has.** A sub-issue does **not** inherit its parent's runner: it
  gets the project default pod like any new issue. Set a dedicated runner
  on a child only if the child itself needs it.

## Run

End an issue run with the `run finish` command, and report a scheduler
run with `run yield`. The end-of-run flow itself — which state to move to —
lives in the prompt section that owns it; this topic is the flag and
result reference.

- `pidash run finish --to <state-name> --result <result> [--comment-file <path>] [--note <line>] [--as-agent <name>] [--run-id <uuid>]` —
  end an issue run in one call: posts the comment (as this agent run),
  moves the issue (pass the current
  state's name to stay) and records the result. Call it once, as your
  **last** `pidash` command.
  - `--result` is one of five: `done` (this stage's work is finished and
    the issue was moved on), `progressed` (did work; more remains in this
    stage — a later run continues), `waiting_on_external` (waiting on
    checks, a merge, or another issue), `waiting_on_human` (a person must
    answer first), `blocked` (cannot proceed until a person acts).
    `waiting_on_human` and `blocked` stop the clock until a person acts;
    the others leave the issue to its stage.
  - `--as-agent` defaults to `PIDASH_AGENT_NAME` inside a run, so the
    closing comment carries the agent's name instead of "AI Agent";
    explicit `--as-agent` wins. `--run-id` defaults to `PIDASH_RUN_ID`.
  - What the server refuses: a run moving its own issue to Done, Cancelled
    or a stage the project switched off. A move into a stage that has agent
    runs switched off is refused with the list of valid states.
  - A repeated call changes nothing: re-running the same call after a
    success is a no-op.
- `pidash run yield --outcome <outcome> [--note <line>] [--stop-ticking] [--run-id <uuid>]` —
  report a scheduler run's outcome. Optional; when you send it, make it
  your **last** `pidash` command. The outcome is recorded on the run for
  the humans reading the schedule's run history. A scheduled run has no
  ticking clock: no outcome changes when the schedule next fires, and
  nothing it reports disables the schedule. `--run-id` defaults to
  `PIDASH_RUN_ID`; only scripted use should need it.

## Context

Two `context.md` files on the dev machine describe the Pi Dash projects
available to you. Both carry a `generated_at` RFC 3339 UTC stamp so you can
judge staleness, and each project entry carries a `repo_url` you can match
against your working directory.

- **Working-dir** `<working_dir>/.pidash/context.md` (`scope: "project"`)
  — the single project this working directory belongs to. This is
  **authoritative for "which project am I working in"**.
- **Machine-level** `~/.pidash/context.md` (`scope: "machine"`) — every
  project in every workspace this machine is bound to, grouped under a
  `workspaces:` list keyed by slug. This is the **lookup table for "what
  else exists in this workspace"** — use it to find a related issue's
  project or file a follow-up in a different project — and the
  **fallback** when there is no working-dir file.

Precedence: trust the working-dir file for your current project; consult
the machine-level file for everything else. If the machine-level file looks
stale or is missing, run `pidash context refresh` to rewrite it for the
bound workspace. (Its path follows `PIDASH_CONFIG_DIR` when set, otherwise
`~/.pidash`.)

- `pidash context init --project <PROJ-or-UUID> [--workspace <path>]` — write the working-dir
  `.pidash/context.md` for the current directory's project. `--workspace`
  writes it for another local directory instead of the current one.
- `pidash context refresh` — rewrite the machine-level `~/.pidash/context.md`
  for the bound workspace.

## Recipes

Short procedures that are neither end-of-run nor blocking (those live in
the prompt sections that own them).

Read your workpad, edit it, write it back. The runner already wrote the
snapshot to `.pidash/workpad.md`, so there is no fetch step:

```sh
pidash workpad update
```

If the file is missing, or an update is rejected as changed:

```sh
pidash workpad get --to-file
```

then re-apply your edit and run `pidash workpad update` again. Update the
workpad when something happens the next run must not lose. An empty file is refused unless you
pass `pidash workpad update --allow-empty` for an intentional clear.

File a finding as a new issue under the scheduler's project:

```sh
pidash issue create --project ENG --title <title> --description-file .pidash/finding.md
```

Write `.pidash/finding.md` first: evidence, file path, severity,
suggested fix (markdown).

## Workspace

- `pidash workspace me` — verify the CLI's Pi Dash credentials end to
  end and print the authenticated user. No flags. When this fails, fix
  the login before debugging any other command.
