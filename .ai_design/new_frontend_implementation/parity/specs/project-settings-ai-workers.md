# Project settings — AI Workers: area spec (NEWFRONT-387)

Row PSET-064. Written from the hybrid assignment design (HA-1,
`.ai_design/dedicated_runner_assignment/design.md` §6.6 and §14.4),
which settles the contract. No old code, strings, styles or assets
reused — the old settings has no such page yet, so this row carries
no oracle result and its scenario runs once the old-app page lands
with PDASHOSS01-299. Until then the frontend builds against the
intended contract: the setting column is optional and absent reads
as the default.

## Lost-runner rule (PSET-064)

The Execution group holds an AI Workers tab at the `/ai-workers`
suffix. It owns one setting: what the project does with a dedicated
issue when its runner is revoked or deleted. The two rules are
failing the queued run and moving the issue to Backlog (the
default), or falling back to the pod so queued runs continue on any
runner there. The tab reads the shared project-settings row behind
a skeleton and saves the rule column through the same project PATCH
as the General tab, re-seeding the cached row so the form, header
and rail agree without a reload; a refused save toasts and keeps
the stored value.

Members see the tab and read the stored rule on a dimmed, inert
form with no save control; project admins edit. Guests are refused
at the page gate like the states and labels tabs.
