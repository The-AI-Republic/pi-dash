# Issue relations from the CLI

`pidash issue relate`, `unrelate` and `relations` let an agent (or a human at
a shell) record and read dependencies between work items — most importantly
`blocked_by`, which the agent run prompt surfaces with each blocker's state.
They use the same auth and workspace scoping as every other `pidash issue`
command.

The chat assistant (`relate_issues` / `unrelate_issues` /
`list_issue_relations`), the Cloud Agent (`pidash_relate_issues` /
`pidash_unrelate_issues` / `pidash_list_issue_relations`) and the hosted MCP
connector use the same relation names, arguments and output shape, all backed
by `apps/api/pi_dash/orchestration/relations.py`.

## Commands

```sh
pidash issue relate ENG-7 --blocked-by ENG-3,ENG-4   # ENG-7 waits on ENG-3 and ENG-4
pidash issue relate ENG-3 --blocking ENG-7           # the same edge, from the other end
pidash issue relate ENG-7 --relates-to ENG-9
pidash issue unrelate ENG-7 --blocked-by ENG-3
pidash issue relations ENG-7                         # grouped list with states
pidash issue get ENG-7                               # carries the same `relations` block
```

Pass exactly one relation flag per call. Each takes a comma-separated list
of identifiers (`ENG-3`) or UUIDs, and names the relation from the first
issue's side:

| Flag                                 | Meaning for `relate A --flag B` | Same edge as              |
| ------------------------------------ | ------------------------------- | ------------------------- |
| `--blocked-by`                       | A cannot finish until B does    | `relate B --blocking A`   |
| `--blocking`                         | B waits on A                    | `relate B --blocked-by A` |
| `--relates-to`                       | loosely related (symmetric)     | `relate B --relates-to A` |
| `--duplicate`                        | duplicates (symmetric)          | `relate B --duplicate A`  |
| `--start-before` / `--start-after`   | start ordering                  | each other's reverse      |
| `--finish-before` / `--finish-after` | finish ordering                 | each other's reverse      |
| `--implemented-by` / `--implements`  | implementation link             | each other's reverse      |

## Behaviour

- **Idempotent.** Relating a pair that already has the relation reports it
  under `unchanged`; unrelating a pair that doesn't have it reports it under
  `not_related`. Neither is an error, so re-running a plan is safe.
- **One relation per pair.** A pair that already carries a _different_
  relation is reported under `conflicts` (with `existing_relation`) and left
  untouched. This also refuses "A blocked_by B" while "B blocked_by A" exists.
  Unrelate first if you mean to change it.
- **All or nothing on lookups.** Every related issue must exist and be in a
  project you are an active member of. If any cannot be found the command
  exits `4` (not found) with the server's `unresolved` list in the error
  detail, and nothing is written.
- **Scoping.** Writing needs Member or Admin on the first issue's project, the
  same as `pidash issue patch`. Related issues may be in other projects in the
  workspace that you can see. Relations to issues you can't see are left out
  of `relations` output.

## Output

`relate`:

```json
{
  "issue": "ENG-7",
  "relation_type": "blocked_by",
  "created": ["ENG-4"],
  "unchanged": ["ENG-3"],
  "conflicts": [],
  "relations": { "blocked_by": [ ... ], "blocking": [], "relates_to": [], ... }
}
```

`unrelate` returns `removed` / `not_related` in place of `created` /
`unchanged` / `conflicts`. `relations` returns `{issue, relations}`.

`relations` always has every type key (`blocked_by`, `blocking`,
`relates_to`, `duplicate`, `start_before`, `start_after`, `finish_before`,
`finish_after`, `implemented_by`, `implements`), each a list of
`{id, identifier, name, state, state_group}` with up to 100 items. Each item
is a superset of the `relations_summary` item (`{identifier, state,
state_group}`) that `pidash issue get` also returns.

## API

The commands call the v1 token API:

| Command     | Request                                                                                                                    |
| ----------- | -------------------------------------------------------------------------------------------------------------------------- |
| `relate`    | `POST /api/v1/workspaces/<slug>/projects/<project>/work-items/<id>/relations/relate/` `{"relation_type", "issues": [...]}` |
| `unrelate`  | `POST .../work-items/<id>/relations/unrelate/` (same body)                                                                 |
| `relations` | `GET .../work-items/<id>/relations/grouped/`                                                                               |

The older `GET`/`POST .../work-items/<id>/relations/` endpoint is unchanged.
