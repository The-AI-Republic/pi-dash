---
name: pidash
description: Use when the user wants to work with Pi Dash from the current coding workspace through the pidash CLI — issues, comments, pages, states, labels, runners, or schedulers.
metadata:
  short-description: Use Pi Dash from a coding agent
---

If `PIDASH_RUN_ID` is set you are inside a Pi Dash agent run: your prompt already has the CLI instructions. Do not load further references; use `pidash docs <topic>` only for commands not covered there.

Use the local `pidash` CLI to work with Pi Dash from the current coding workspace. Full reference: `pidash docs <topic>` (topics: issue, comment, workpad, page, label, state, scheduler, pods, run, context, recipes). Flags: `pidash <command> --help`.

```bash
pidash --version
pidash workspace me
```

If `pidash` is missing or `workspace me` fails with an auth error (exit 3), ask the user to run `pidash auth login`. Do not log in yourself.

| Goal                    | Command                                                                                               |
| ----------------------- | ----------------------------------------------------------------------------------------------------- |
| Read an issue           | `pidash issue get PROJ-123`                                                                           |
| List / search           | `pidash issue list --project PROJ` / `pidash issue search "<query>" --project PROJ`                   |
| Create                  | `pidash issue create --project PROJ --title "..." --description-file <path>`                          |
| Move                    | `pidash issue patch PROJ-123 --state "In Progress"`                                                   |
| Comment                 | `pidash comment list PROJ-123` / `pidash comment add PROJ-123 --body-file <path>`                     |
| Workpad                 | `pidash workpad get` / `pidash workpad update`                                                        |
| States / labels / pages | `pidash state list PROJ-123` / `pidash label list --project PROJ` / `pidash page list --project PROJ` |
| Schedulers / runners    | `pidash scheduler list --project PROJ` / `pidash project runners --project PROJ`                      |
| Link a PR               | `pidash issue attach-review PROJ-123 --url <url>`                                                     |

Output is JSON — parse it, do not scrape it. Long text goes through a file, never inline. Search before creating, to avoid duplicates. `--label` replaces the whole set on `issue patch`; `--add-label` / `--remove-label` keep what a human set. Confirm destructive or shared writes first.
