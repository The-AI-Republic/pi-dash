# 17 — `pidash` CLI Reference

Complete reference for the `pidash` binary. All commands, all flags, brief examples.

For the _guided_ setup walkthrough use [02 — Cloud Quickstart](./02-cloud-quickstart.md). For the runner's internals see [07 — Runner architecture](./07-runner-architecture.md).

## Global flags

Available on every command:

| Flag                  | Env                 | Default        | Purpose                                              |
| --------------------- | ------------------- | -------------- | ---------------------------------------------------- |
| `--config-dir <PATH>` | `PIDASH_CONFIG_DIR` | XDG config dir | Override config location                             |
| `--data-dir <PATH>`   | `PIDASH_DATA_DIR`   | XDG data dir   | Override data location                               |
| `--log <LEVEL>`       | `PIDASH_LOG`        | `info`         | Log level: `trace`, `debug`, `info`, `warn`, `error` |
| `--help`              | —                   | —              | Show help for any command                            |
| `--version`           | —                   | —              | Print version                                        |

## `pidash` (no subcommand)

First-run launcher. If no config exists, drops into `pidash auth login`. Otherwise prints help.

---

## Setup & auth

### `pidash auth login`

Browser-based device-code login. Same UX as `gh auth login` / `stripe login`.

```
pidash auth login [--url <URL>] [--no-browser] [--no-runner-prompt]
```

| Flag                 | Purpose                                                                                    |
| -------------------- | ------------------------------------------------------------------------------------------ |
| `--url <URL>`        | Cloud base URL (e.g. `https://pidash.airepublic.com`). Optional if config already has one. |
| `--no-browser`       | Don't try to open the verification URL automatically.                                      |
| `--no-runner-prompt` | Skip the post-login "add a runner now?" prompt.                                            |

Writes the CLI token to `config.toml`. On a TTY with no runners yet, offers to register one inline.

### `pidash auth status`

Show who you're logged in as and which runners are registered on this host.

```
pidash auth status
```

### `pidash auth logout`

Revoke the CLI token server-side and clear it locally. `[[runner]]` blocks are left intact — the daemon keeps running under its own identity.

```
pidash auth logout [--local-only]
```

| Flag           | Purpose                                                                            |
| -------------- | ---------------------------------------------------------------------------------- |
| `--local-only` | Skip server-side revoke, just clear local token. Useful when cloud is unreachable. |

### `pidash connect` _(legacy enrollment-token flow)_

Original pairing flow. Use only when device-code login isn't an option (headless / scripted hosts). Generate the token in the cloud UI: **Runners → Add connection**.

```
pidash connect --url <URL> --token <TOKEN> [--host-label <LABEL>]
               [--working-dir <PATH>] [--agent codex|claude-code]
               [--skip-service] [--skip-linger]
```

| Flag                   | Purpose                                                               |
| ---------------------- | --------------------------------------------------------------------- |
| `--url <URL>`          | Cloud base URL (required).                                            |
| `--token <TOKEN>`      | One-time enrollment token (required, single-use).                     |
| `--host-label <LABEL>` | Free-form host label. Defaults to hostname.                           |
| `--working-dir <PATH>` | Runner's working dir. Defaults to `data_dir/runners/<rid>/workspace`. |
| `--agent <KIND>`       | `codex` or `claude-code`. Defaults to `codex`.                        |
| `--skip-service`       | Skip the post-enroll doctor + service install (CI).                   |
| `--skip-linger`        | Skip `loginctl enable-linger` on Linux (avoid sudo prompt).           |

---

## Local config

### `pidash config set default-project <PROJECT>`

Set the local default project for non-interactive CLI calls (e.g. `pidash issue list` without `--project`).

```
pidash config set default-project <PROJECT_IDENTIFIER_OR_UUID>
```

---

## Runner management

### `pidash runner add`

Register a new runner. Uses the CLI token from `pidash auth login`.

```
pidash runner add --project <PROJECT> [--name <NAME>] [--workspace <SLUG>]
                  [--pod <POD>] [--working-dir <PATH>] [--agent codex|claude-code]
```

| Flag                | Purpose                                                            |
| ------------------- | ------------------------------------------------------------------ |
| `--project <P>`     | Project identifier (slug or UUID). **Required.**                   |
| `--name <N>`        | Human-friendly runner name. Auto-generated if omitted.             |
| `--workspace <S>`   | Workspace slug. Required if you belong to multiple workspaces.     |
| `--pod <P>`         | Pod within the project. Defaults to project's default pod.         |
| `--working-dir <P>` | Local working dir for clones. Defaults to a path under `data_dir`. |
| `--agent <K>`       | `codex` (default) or `claude-code`.                                |

On the first runner: installs the OS service (systemd user unit / launchd agent / Windows scheduled task) and starts the daemon.

### `pidash runner list`

List runners configured on this machine.

```
pidash runner list
```

### `pidash runner remove`

Deregister a runner (cloud + local).

```
pidash runner remove <NAME> [--local-only] [-y|--yes]
```

| Flag           | Purpose                                                                            |
| -------------- | ---------------------------------------------------------------------------------- |
| `<NAME>`       | Runner name from `pidash runner list`. Must match a `[[runner]]` in `config.toml`. |
| `--local-only` | Skip cloud-side delete; just clean local config + data dir.                        |
| `-y`, `--yes`  | Skip the y/N confirm. Required for scripted callers.                               |

---

## Service lifecycle

### `pidash install`

Write or refresh the OS service unit. Safe to re-run after binary upgrades.

```
pidash install [--skip-linger]
```

| Flag            | Purpose                                                                                                       |
| --------------- | ------------------------------------------------------------------------------------------------------------- |
| `--skip-linger` | Linux only — skip `sudo loginctl enable-linger`. Without linger the daemon only starts at login, not at boot. |

### `pidash uninstall`

Remove the OS service unit. Local config + runner credentials are kept.

```
pidash uninstall
```

### `pidash start` / `pidash stop` / `pidash restart`

Control the installed service.

```
pidash start
pidash stop
pidash restart
```

### `pidash status`

Combined service-level + daemon-runtime status.

```
pidash status [--json]
```

| Flag     | Purpose                                |
| -------- | -------------------------------------- |
| `--json` | Print JSON instead of a human summary. |

If the daemon isn't running, prints just the service-level state and a note (instead of an IPC error).

### `pidash tui`

Attach the interactive Ratatui UI to the running daemon.

```
pidash tui [--tab <TAB>]
```

| Flag          | Purpose                                                                                                      |
| ------------- | ------------------------------------------------------------------------------------------------------------ |
| `--tab <TAB>` | Open directly to a tab: `runner` (default), `config`, `runs`, or `approvals`. Accepts 1-based index `1`–`4`. |

Press `q` to quit; the daemon keeps running in the background.

### `pidash doctor`

Preflight checks: agent installed + on `PATH`, agent authed, git configured, cloud reachable.

```
pidash doctor [--json] [--runner <NAME>]
```

| Flag              | Purpose                                                                  |
| ----------------- | ------------------------------------------------------------------------ |
| `--json`          | Machine-readable report.                                                 |
| `--runner <NAME>` | Restrict per-runner checks to one runner. Daemon-wide checks always run. |

### `pidash update`

Swap the on-disk `pidash` binary for the latest GitHub release. Only works for cargo-dist installs (those leave an install receipt).

```
pidash update [--check] [--restart]
```

| Flag        | Purpose                                                                    |
| ----------- | -------------------------------------------------------------------------- |
| `--check`   | Report whether an update is available, don't swap.                         |
| `--restart` | After swap, restart the daemon so the new binary takes effect immediately. |

Without `--restart`, the swap only takes effect on the next natural restart (`pidash restart`, reboot, service respawn). The running process is never disturbed.

For a one-command manual upgrade, run `pidash update --restart` when the runner is idle.

### `pidash remove --all`

Full teardown: stop service → uninstall service unit → deregister with cloud → delete local `config.toml` + credentials.

```
pidash remove --all [--local-only]
```

| Flag           | Purpose                                                                             |
| -------------- | ----------------------------------------------------------------------------------- |
| `--all`        | **Required.** Explicit confirmation that you want to wipe ALL runners on this host. |
| `--local-only` | Delete local state without contacting the cloud.                                    |

> To drop a single runner instead, use `pidash runner remove <name>`.

---

## Workspace / project / context

### `pidash workspace me`

Verify CLI credentials end-to-end (token + cloud reachability + TLS + active user). The probe `pidash doctor` uses.

```
pidash workspace me
```

### `pidash project list`

List projects in the active workspace. Prints JSON.

```
pidash project list
```

### `pidash context init`

Write `.pidash/context.md` for a local workspace directory.

```
pidash context init --project <PROJECT> [--workspace <PATH>]
```

| Flag              | Purpose                                             |
| ----------------- | --------------------------------------------------- |
| `--project <P>`   | Project identifier or UUID. **Required.**           |
| `--workspace <P>` | Local workspace dir. Defaults to current directory. |

---

## Work items

### `pidash issue get <IDENTIFIER>`

Fetch one work item. Prints full JSON.

```
pidash issue get ENG-42
```

### `pidash issue create`

Create a work item under a project.

```
pidash issue create --title <TITLE> [--project <P>] [--description <D>]
                    [--priority <P>] [--state <S>]
```

| Flag                | Purpose                                                                                                        |
| ------------------- | -------------------------------------------------------------------------------------------------------------- |
| `--title <T>`       | **Required.**                                                                                                  |
| `--project <P>`     | Project identifier or UUID. If omitted: `PIDASH_PROJECT_ID` env → local `default_project` → workspace default. |
| `--description <D>` | Plain text or markdown.                                                                                        |
| `--priority <P>`    | `none` \| `low` \| `medium` \| `high` \| `urgent`.                                                             |
| `--state <S>`       | Initial state — name (case-insensitive) or UUID.                                                               |

### `pidash issue list --project <P>`

List work items in a project. Returns the paginated envelope `{count, next_cursor, prev_cursor, results}`.

```
pidash issue list --project <P> [--cursor <C>] [--per-page <N>] [--order-by <F>]
```

| Flag             | Purpose                                                              |
| ---------------- | -------------------------------------------------------------------- |
| `--project <P>`  | **Required.**                                                        |
| `--cursor <C>`   | Pagination cursor from a prior page's `next_cursor`.                 |
| `--per-page <N>` | Items per page. Server default if omitted.                           |
| `--order-by <F>` | Sort field, e.g. `-created_at` (default), `priority`, `state__name`. |

### `pidash issue patch <IDENTIFIER>`

Update fields. Pass only the fields you want to change.

```
pidash issue patch ENG-42 [--state <S>] [--title <T>] [--description <D>] [--priority <P>]
```

| Flag                | Purpose                                            |
| ------------------- | -------------------------------------------------- |
| `--state <S>`       | State name (case-insensitive) or UUID.             |
| `--title <T>`       | New title.                                         |
| `--description <D>` | New description.                                   |
| `--priority <P>`    | `none` \| `low` \| `medium` \| `high` \| `urgent`. |

### `pidash issue move <IDENTIFIER> --project <P>`

Move a work item into another project in the same workspace.

```
pidash issue move ENG-42 --project OPS
```

---

## Comments

### `pidash comment list <IDENTIFIER>`

List comments on a work item.

```
pidash comment list ENG-42
```

### `pidash comment add <IDENTIFIER>`

Post a new comment.

```
pidash comment add ENG-42 (--body <BODY> | --body-file <PATH>)
```

| Flag                 | Purpose                                |
| -------------------- | -------------------------------------- |
| `--body <B>`         | Comment body (plain text or markdown). |
| `--body-file <PATH>` | Read body from file.                   |

### `pidash comment update <IDENTIFIER> <COMMENT_UUID>`

Edit an existing comment owned by you.

```
pidash comment update ENG-42 <COMMENT_UUID> (--body <BODY> | --body-file <PATH>)
```

---

## States

### `pidash state list [PROJECT_OR_ISSUE]`

List workflow states for a project. Without an argument, uses the current issue context from `PIDASH_ISSUE_IDENTIFIER`.

```
pidash state list             # uses current issue context
pidash state list ENG         # project slug
pidash state list ENG-42      # via work-item identifier
pidash state list <UUID>      # project UUID
```

---

## Hidden / internal

### `pidash __run`

Internal daemon entry point. Invoked by systemd / launchd / Windows scheduled task via the generated unit file. **Not a user-facing command.**

---

## Exit codes

`issue`, `comment`, `state`, `workspace`, `project`, `context` print JSON on stdout and JSON on stderr for errors. Their exit codes follow `api_client::EXIT_*` constants — non-zero on any error.

Other commands return `0` on success and surface anyhow errors on failure (non-zero exit).

## Common environment variables

| Variable                  | Used by        | Purpose                                  |
| ------------------------- | -------------- | ---------------------------------------- |
| `PIDASH_CONFIG_DIR`       | All            | Override config dir                      |
| `PIDASH_DATA_DIR`         | All            | Override data dir                        |
| `PIDASH_LOG`              | All            | Log level                                |
| `PIDASH_PROJECT_ID`       | `issue create` | Default project when `--project` omitted |
| `PIDASH_ISSUE_IDENTIFIER` | `state list`   | Default issue context                    |

## Common file locations

| What                   | Linux / macOS                               | Windows                                     |
| ---------------------- | ------------------------------------------- | ------------------------------------------- |
| Config + CLI token     | `~/.config/pidash/config.toml`              | `%APPDATA%\pidash\config.toml`              |
| Per-runner credentials | `<data_dir>/runners/<rid>/credentials.toml` | `<data_dir>\runners\<rid>\credentials.toml` |
| Run transcripts        | `<data_dir>/runners/<rid>/history/`         | `<data_dir>\runners\<rid>\history\`         |
| IPC socket / pipe      | `$XDG_RUNTIME_DIR/pidash/...`               | local named pipe                            |

All secrets and the IPC socket are `0600` on Unix.
