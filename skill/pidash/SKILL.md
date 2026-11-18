---
name: pidash
description: Use when the user wants to work with Pi Dash from the current coding workspace through the pidash CLI — create, read, update, list, search, move, label, or relate issues (work items); post or read issue comments; attach a pull request or merge request to an issue; read or write project pages; inspect workflow states, labels, runners, or schedulers; start an agent run on an issue; or connect the current repo to a Pi Dash project.
metadata:
  short-description: Use Pi Dash from a coding agent
---

# Pi Dash

Use the local `pidash` CLI to work with Pi Dash from the current coding workspace.

Read `references/pidash-workflows.md` before the first write (create, patch, move, comment, relate) in a session. It carries the full command reference; this file is the short version.

## Preflight

On first use in a session:

```bash
pidash --version
pidash workspace me
```

If `pidash` is missing or `workspace me` fails with an auth error (exit code 3), ask the user to run `pidash auth login`. Do not try to log in yourself — it is an interactive device-code flow.

## Conventions

- **`--help` is the source of truth.** The CLI changes faster than this skill. Before using a subcommand for the first time, or when a flag is rejected, run `pidash <command> <subcommand> --help`. If a command described here does not exist, the installed CLI is older — tell the user they can upgrade with `pidash update`.
- **Output is JSON on stdout** (except `pidash ai` without `--json`, and `page get --body-only`). Parse it; do not scrape it. Errors are one JSON object on stderr, `{"error": "...", "detail": "..."}`, with an exit code: `2` invalid input, `3` auth, `4` not found, `5` server error, `6` throttled, `1` anything else.
- **Issues are addressed as `PROJ-123`.** Projects are addressed by identifier (`PROJ`) or UUID. States, labels, pods, and runners accept a name (case-insensitive) or a UUID.
- **Long text goes through a file.** Use `--description-file` and `--body-file` instead of inlining multi-line markdown in a shell argument.
- **Throttled (exit 6) means wait, not retry in a loop.**
- **Empty bodies are refused.** `comment update`, `page update`, and `workpad update` reject an empty body unless `--allow-empty` is passed. Do not pass it to get past an error — an empty file usually means the step before it failed.
- **Setup is the user's.** `pidash auth status` and `pidash doctor` are safe to run when something looks wrong. Do not run `pidash update`, `pidash runner ...`, or the service commands (`install`, `start`, `stop`, `restart`, `remove`) on your own initiative.

## Project Context

The project for this working directory is recorded in `.pidash/context.md` at the workspace root. If it exists, it is authoritative: use it and do not ask the user which project to use.

If it is missing:

1. Run `pidash project list`. Each project carries `identifier`, `name`, `description`, `repo_url`, and `is_default`.
2. Match `repo_url` against `git remote get-url origin`. Fall back to name and description against the repo name and README.
3. Ask the user to confirm the candidate, then record it:

```bash
pidash context init --project <identifier-or-uuid>
```

`~/.pidash/context.md` is a machine-level lookup of the projects in every workspace this machine is bound to — useful for "what other projects exist", not for deciding the current one. `pidash context refresh` updates it for the current workspace.

The CLI does not read `.pidash/context.md` — you do, and you pass its project as `--project`. When a command is run without `--project`, the CLI falls back to `PIDASH_PROJECT_ID`, then the local default (`pidash config set default-project`), then the workspace default project. Without a human to ask, never guess a project: if none of these resolves, stop and report the setup step.

## Everyday Commands

| Goal                       | Command                                                                                        |
| -------------------------- | ---------------------------------------------------------------------------------------------- |
| Read an issue              | `pidash issue get PROJ-123`                                                                    |
| Read its conversation      | `pidash comment list PROJ-123`                                                                 |
| Find issues by content     | `pidash issue search "<query>" [--project PROJ] [--status open]`                               |
| List issues by filter      | `pidash issue list --project PROJ [--state-group open] [--label bug] [--fields ...]`           |
| Create an issue            | `pidash issue create --project PROJ --title "..." --description-file <path>`                   |
| Change state or fields     | `pidash issue patch PROJ-123 --state "In Progress"`                                            |
| Add or remove a label      | `pidash issue patch PROJ-123 --add-label bug` / `--remove-label bug`                           |
| Move to another project    | `pidash issue move PROJ-123 --project OTHER`                                                   |
| Record a blocker           | `pidash issue relate PROJ-7 --blocked-by PROJ-3,PROJ-4`                                        |
| Comment as the agent       | `pidash comment add PROJ-123 --body-file <path> --as-agent "<runtime name>"`                   |
| Link a PR or MR            | `pidash issue attach-review PROJ-123 --url <url>`                                              |
| Valid states for an issue  | `pidash state list PROJ-123`                                                                   |
| Valid labels for a project | `pidash label list --project PROJ`                                                             |
| Read the project wiki      | `pidash page list --project PROJ`, then `pidash page get <page-id> --project PROJ --body-only` |

## Rules That Matter

**Search before creating.** Run `pidash issue search` on the title's key phrase first. If a strong match exists, show it to the user and ask whether to update it instead of filing a duplicate.

**The title is the TL;DR for a human.** Someone who has not seen your session or the code should understand from the title alone what is wrong or wanted. Describe the effect a user or operator sees, not the mechanism: "Pi Dash can stay down until next login on Windows if another app takes its pipe name", not "named-pipe squatting (first-instance create) keeps daemon down". Plain words, about 80 characters at most, no file paths, function names, or parenthesized internals. Technical detail goes in the description, which can start with a one-line technical summary. The same applies when you rewrite a title with `issue patch --title`. See [Writing the title](references/pidash-workflows.md#writing-the-title).

**Say which project you used.** After creating an issue, report it: `Created PROJ-123 in project <project name>.` If the user corrects the project, `pidash issue move` it rather than recreating it.

**Reuse the project's vocabulary.** Check `pidash state list PROJ-123` before setting a state and `pidash label list` before attaching a label. Labels must already exist; create one with `pidash label create` only when nothing fits.

**`--label` replaces, `--add-label` appends.** On `issue patch`, `--label a,b` detaches every label not listed. To change one label, use `--add-label` / `--remove-label`.

**Lists are paginated.** `issue list` returns one page. When `next_page_results` is `true` the list is truncated — pass `next_cursor` back with `--cursor`. Use `--fields` to keep each item small. Recent CLIs default `issue list` to open issues; pass `--state-group` explicitly when you need closed ones.

**Mark your own comments.** Always pass `--as-agent` with your runtime name (`Claude Code`, `Codex`, otherwise `AI Agent`) when the comment is your message, and add `--agent-run-id "$PIDASH_RUN_ID"` when that variable is set. The authenticated CLI user is only the audit principal; `--as-agent` is what tells humans and later agents who spoke.

**A pull request needs two steps.** After opening a PR or MR for an issue: (1) comment the link on the issue, because comments are how the human follows the work, and (2) `pidash issue attach-review <identifier> --url <url>` so Pi Dash tracks its status. Neither replaces the other.

**Confirm before destructive or shared writes.** `pidash label delete` detaches the label everywhere with no undo. Scheduler writes (`install`, `update`, `run-now`, `uninstall`, `create`, `edit`, `delete`) and `pidash issue run-ai` affect the whole project and spend agent budget — do them only when the user asked for exactly that.

## Inside a Pi Dash Agent Run

`pidash workpad`, `pidash run yield`, `pidash issue wait`, and `pidash issue patch --git-work-branch` exist for agents that Pi Dash itself dispatched (`PIDASH_RUN_ID` is set). The run's own prompt tells that agent when to call them. In an ordinary interactive session, do not call them.
