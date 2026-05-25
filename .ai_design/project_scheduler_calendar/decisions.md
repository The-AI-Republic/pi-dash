# Project Scheduler Calendar — Decisions Log

**Status:** Draft
**Date:** 2026-05-24
**Scope:** Move `SchedulerBinding.cron` to iCal-shaped recurrence; add a Google-Calendar-style project-level view of past + future scheduler firings.

Related:

- `../project_scheduler/design.md` — original scheduler design (cron-based)
- `../project_scheduler/tasks.md` — original PR breakdown

This is a **decisions log**, not a design narrative — Q&A format covering the
calls that need making before implementation. Where the answer has a rationale
worth preserving, it's spelled out; otherwise just the call. PR-level tasks
will live in `tasks.md` (to be written when implementation starts).

---

## 0. Locked product decisions (from prior conversation)

- **Three PRs in sequence**, each independently shippable:
  - **PR1**: iCal recurrence schema + `Scheduler.color` + parser swap. Backend + minimal frontend (modal + list column).
  - **PR2**: occurrences endpoint. Backend only.
  - **PR3**: project-level calendar UI ("Option C" — copy-adapt from the existing issue calendar).
- **iCal recurrence is the storage model.** `cron` is dropped in PR1 (no expand-then-contract); mitigated by a dry-run management command.
- **`Scheduler.color` ships in PR1** (new column on the workspace-level template), not a frontend-only palette.
- **Calendar is project-level only** for v1. Workspace-level overview deferred.
- **Calendar shows past + future** in one view. Past = grey (status surfaced on
  click/hover); future = colored by scheduler.
- **Time-axis week view is in v1 scope** (not a stretch). New visual primitive
  the issue calendar doesn't have.
- **Multi-binding in one view** with a Google-Calendar-style toggle rail.
- **Tab bar on the schedulers page** (List | Calendar). No separate sidebar entry.
- **Parent GitHub tracking issue** linking PR1→PR2→PR3.
- **Depends on PR #153** (project sidebar Schedulers entry) being merged into
  `main` first — that PR is open and is a prerequisite for the page surface this
  calendar tab attaches to.

---

## 1. Schema & migration (PR1)

### Q: What columns does `SchedulerBinding` lose and gain?

**A:** Drop `cron` (CharField). Add:

| Column    | Type                                      | Notes                                                                                                   |
| --------- | ----------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `dtstart` | `DateTimeField(null=False)`               | Anchor for the series. Tz-aware (stored UTC).                                                           |
| `tzid`    | `CharField(max_length=64, default="UTC")` | IANA timezone name (e.g. `America/Los_Angeles`). Used at expansion time for wall-clock-aware semantics. |
| `rrule`   | `TextField(blank=True, default="")`       | RFC 5545 RRULE string. Empty = single-shot at `dtstart`. May contain `UNTIL` or `COUNT`.                |
| `rdates`  | `JSONField(default=list)`                 | Extra one-off firings (list of ISO datetime strings).                                                   |
| `exdates` | `JSONField(default=list)`                 | Firings to skip (list of ISO datetime strings).                                                         |

`next_run_at`, `last_run`, `last_error`, `extra_context`, `enabled`, `actor` all
unchanged.

### Q: What does `Scheduler` gain?

**A:** One column:

| Column  | Type                                         | Notes                                                                                                          |
| ------- | -------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `color` | `CharField(max_length=7, default="#3b82f6")` | Hex RGB. Chosen from a fixed 16-color palette (see §6) but stored as the hex so freeform values are tolerated. |

### Q: How are existing `cron` values migrated?

**A:** Hand-rolled converter in `pi_dash/db/migrations/0140_scheduler_binding_rrule.py`
(data migration). The 5-field cron universe is small; covers `*`, `N`, `N-M`,
`*/N`, comma-separated lists across `minute hour day-of-month month day-of-week`.
Produces an RRULE per cron. Examples:

| cron           | RRULE                                                   |
| -------------- | ------------------------------------------------------- |
| `0 9 * * 1-5`  | `FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;BYHOUR=9;BYMINUTE=0`  |
| `*/30 * * * *` | `FREQ=MINUTELY;INTERVAL=30`                             |
| `0 0 1 * *`    | `FREQ=MONTHLY;BYMONTHDAY=1;BYHOUR=0;BYMINUTE=0`         |
| `15 14 * * 3`  | `FREQ=WEEKLY;BYDAY=WE;BYHOUR=14;BYMINUTE=15`            |
| `0 22 * * 1-5` | `FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;BYHOUR=22;BYMINUTE=0` |

### Q: Default `dtstart` for migrated bindings?

**A:** `binding.created_at` rounded **forward** to the next valid cron firing.
Rationale: preserves "next fire" continuity — if cron said it next fires at
`2026-05-25T09:00:00Z`, the new RRULE anchored at that instant fires at the
same moment. No phase shift on deploy.

### Q: Default `tzid` for migrated bindings?

**A:** `"UTC"`. Existing cron expressions were evaluated in UTC, so this
preserves behavior. Users can edit `tzid` later for new DST-aware semantics.

### Q: Cron expressions the converter can't handle?

**A:** Log to a dedicated table or stdout in the dry-run command. For each
unparseable cron in production data: manually decide a translation (or disable
the binding) and add it to the migration data file as a hardcoded mapping
before the migration runs. The realistic universe is small (we don't run
Jenkins-style hash crons), and the dry-run command will list every binding
ahead of time.

### Q: Bindings whose `dtstart` falls before "now" after migration?

**A:** That's fine — `dateutil.rrule.after(now)` skips past-due occurrences and
returns the first future one. The series anchor is conceptual, not "the first
firing."

### Q: Keep `cron` column nullable for one release (expand-then-contract)?

**A:** **No.** User chose to drop in same migration. Mitigation = dry-run
management command (`manage.py dry_run_scheduler_migration`) that prints
old-cron next-fire vs new-rrule next-fire for every binding and flags
discrepancies; we run it in dev and post the diff to the PR description for
human review before merge.

---

## 2. RRULE constraints (PR1)

### Q: Allowed `FREQ` values?

**A:** `MINUTELY`, `HOURLY`, `DAILY`, `WEEKLY`, `MONTHLY`, `YEARLY`. **Reject
`SECONDLY`** — it's a denial-of-service vector against the per-minute Beat tick
and has no use case here.

### Q: Minimum interval?

**A:** Effective fire rate must be ≥ 1 minute. `FREQ=MINUTELY` with no
`INTERVAL` (= every 1 min) is the floor. `FREQ=MINUTELY;INTERVAL=0` is invalid
per spec; reject. Beat ticks per minute, so sub-minute resolution doesn't fire
anyway.

### Q: Max recurrence depth?

**A:** No artificial depth cap. The combination of FREQ whitelist + 1-min
minimum interval bounds the expansion cost. Belt-and-suspenders: the
occurrences endpoint caps response size at 5000 (§3), so a pathological RRULE
gets truncated server-side.

### Q: Validation pipeline?

**A:** On install/edit POST/PATCH:

1. Parse with `dateutil.rrule.rrulestr(rrule_str, dtstart=dtstart)` — catches
   syntax errors.
2. Inspect parsed rule's `_freq` — reject if `SECONDLY`.
3. Reject if `_interval` produces sub-minute firing.
4. Validate `rdates`/`exdates` are well-formed ISO datetime strings.

Failures return HTTP 400 with `{"error": "invalid_rrule", "detail": "<message>"}`.

### Q: DST handling?

**A:** Use `dateutil.rrule` with a tz-aware `dtstart` built from `tzid`. The
library handles DST natively — a 9am-PT binding fires at "9am wall clock" on
both sides of the transition. On the spring-forward day where 9am doesn't
exist, dateutil returns the next valid local instant. On fall-back where 9am
happens twice, dateutil fires once. Document this behavior in scheduler
tooltip help text; don't try to invent custom policy.

### Q: Behavior when a stored RRULE is somehow invalid at runtime?

**A:** Same `last_error` + auto-disable path the current bad-cron handler uses
(`apps/api/pi_dash/bgtasks/scheduler.py:188`). No new behavior.

---

## 3. Occurrences endpoint (PR2)

### Q: URL?

**A:** `GET /api/workspaces/<slug>/projects/<project_id>/scheduler-bindings/occurrences/?from=<ISO>&to=<ISO>`

### Q: Response shape?

**A:**

```json
{
  "occurrences": [
    {
      "binding_id": "uuid",
      "scheduler_id": "uuid",
      "scheduler_name": "Security audit",
      "scheduler_color": "#3b82f6",
      "dtstart": "2026-05-26T09:00:00Z",
      "tzid": "America/Los_Angeles",
      "kind": "scheduled",
      "agent_run_id": null,
      "status": null
    },
    {
      "binding_id": "uuid",
      "scheduler_id": "uuid",
      "scheduler_name": "Security audit",
      "scheduler_color": "#3b82f6",
      "dtstart": "2026-05-24T09:02:14Z",
      "tzid": "America/Los_Angeles",
      "kind": "past",
      "agent_run_id": "uuid",
      "status": "success"
    }
  ],
  "has_more": false,
  "next_window_start": null
}
```

`kind`: `"scheduled"` (future, expanded from RRULE) or `"past"` (real
`AgentRun` row). `status` populated for past only; one of
`success | failed | cancelled | running`.

### Q: Max window size per request?

**A:** **90 days**. Covers a 3-month calendar view comfortably; longer windows
force the client to make multiple requests (natural pagination on the date
axis). Reject `to - from > 90 days` with HTTP 400.

### Q: Hard cap on occurrence count?

**A:** 5000 per response. Above that: truncate, set `has_more: true`,
`next_window_start = <dtstart of the first dropped occurrence>`. Client can
request the next chunk if it cares. Default UI just shows the cap and a "+N
more — narrow the date range" hint.

### Q: Caching?

**A:**

- Past slice: cache key `(workspace, project, from, to, max(agent_run_id))` with
  5-min TTL in Valkey. Past occurrences don't change once written.
- Future slice: cache key `(workspace, project, from, to)` with 30-second TTL.
  Bust on any binding mutation in that project.
- Past + future are computed separately and merged at response time.

### Q: Authz?

**A:** Same gate as `ProjectSchedulerBindingListEndpoint.get` —
`ROLE.ADMIN | ROLE.MEMBER | ROLE.GUEST`, level `"PROJECT"`. Project scope is
enforced by the URL params + the standard `workspace.slug = url.slug` join.

### Q: Disabled scheduler / disabled binding behavior?

**A:**

- `Scheduler.is_enabled = False` → omit all that scheduler's occurrences from
  both past and future slices. Disabled schedulers don't fire, so showing them
  is misleading.
- `SchedulerBinding.enabled = False` → omit future occurrences, **include past
  AgentRuns** (a binding may have been enabled at fire time, disabled later;
  history is real).

---

## 4. Calendar UI structure (PR3)

### Q: Directory layout?

**A:** `apps/web/core/components/project/scheduler-bindings/calendar/`

```
calendar/
  index.tsx                          # SchedulerCalendar root
  header.tsx                         # month nav + month/week toggle + date jumper
  calendars-rail.tsx                 # right-side panel: scheduler swatches + visibility toggles
  month-view/
    month-grid.tsx                   # copied + adapted from issue calendar week-days.tsx
    day-tile.tsx                     # copied + adapted from issue calendar day-tile.tsx
    occurrence-block.tsx             # month-view block (color + label)
  week-view/
    week-axis.tsx                    # NEW — vertical hour ruler + day columns
    time-column.tsx                  # hour labels left rail
    day-column.tsx                   # one column per day, holds positioned blocks
    occurrence-block.tsx             # week-view block (positioned by time)
    current-time-line.tsx            # red line at "now"
  occurrence-drawer.tsx              # opens on click — past shows AgentRun detail, future shows binding edit
  hooks/
    use-occurrences.ts               # SWR hook against the PR2 endpoint
    use-visible-schedulers.ts        # state for which schedulers are toggled on
    use-calendar-date-range.ts       # derived from month/week + selected date
```

### Q: Route shape?

**A:**

```
/<workspaceSlug>/projects/<projectId>/schedulers          # existing List tab (PR #153)
/<workspaceSlug>/projects/<projectId>/schedulers/calendar # new Calendar tab
```

Layout file at
`apps/web/app/(all)/[workspaceSlug]/(projects)/projects/(detail)/[projectId]/schedulers/layout.tsx`
hosts a tab bar. The current `page.tsx` becomes the List tab content; new
`calendar/page.tsx` becomes the Calendar tab content. Route registered in
`apps/web/app/routes/core.ts`.

### Q: How do we pick which view (month vs week)?

**A:** Query param `?view=month` (default) or `?view=week`. URL is the source
of truth so deep links work. User-preference persistence (last-used view) is
deferred.

---

## 5. Time-axis week view (the new bit)

### Q: Hour-row height?

**A:** 48px (matches Google Calendar default). Total view height = 24 × 48 =
1152px scrollable.

### Q: Initial scroll position?

**A:** On first mount: scroll-snap so the current hour is the second row from
the top (i.e. show one hour of "past today" + the rest of the day). Subsequent
mounts: restore previous scroll position if same week, else reset.

### Q: Day column width?

**A:** Flex equal across visible days. 7 days when `showWeekends=true`, 5
otherwise. Time column on left fixed at 64px.

### Q: Current-time line?

**A:** 2px red horizontal line spanning all day columns, positioned by
`(now.minutes / 60) * 48`. Updates every 60 seconds via `setInterval`. Visible
only on the column for "today."

### Q: Occurrence block dimensions in week view?

**A:** Width = full column minus 4px padding. Height = fixed 20px (since
occurrences have no duration — they're point-in-time triggers). Vertical
position = `(occurrence.local_minutes / 60) * 48`.

### Q: Block content in week view?

**A:** Single line: `[swatch] HH:MM scheduler-name`. Truncate scheduler name
with ellipsis. On hover: tooltip with full name, status (past), or "scheduled"
(future).

### Q: Block content in month view?

**A:** Same: `[swatch] HH:MM scheduler-name`, stacked vertically inside the
day tile. Identical to how the issue calendar stacks issue blocks per day.

### Q: Time format?

**A:** `Intl.DateTimeFormat(locale, { hour: "numeric", minute: "2-digit" })`.
Renders 12-hour with AM/PM in `en-US`, 24-hour in most other locales. Honors
the user's browser locale.

### Q: Timezone display?

**A:** Render occurrences in the user's local browser timezone. If
`occurrence.tzid !== user_tz`, the drawer shows both:
"Tue May 26, 9:00 AM PDT (your time) — Tue May 26, 12:00 PM EDT (binding time)."
Tooltip on the block also includes the tzid suffix when they differ.

---

## 6. Color

### Q: Palette?

**A:** Fixed 16-color palette, hex values stored as `Scheduler.color`. Chosen
from Tailwind's -500 weight for legibility on both light and dark themes:

```
#3b82f6 blue    #6366f1 indigo  #8b5cf6 violet   #a855f7 purple
#d946ef fuchsia #ec4899 pink    #ef4444 red      #f97316 orange
#eab308 yellow  #84cc16 lime    #22c55e green    #10b981 emerald
#14b8a6 teal    #06b6d4 cyan    #0ea5e9 sky      #f59e0b amber
```

Stored as the hex string. Picker UI on scheduler create/edit shows the 16
swatches; clicking sets the column. Freeform hex input allowed but not
surfaced in the picker (advanced users can `PATCH` directly).

### Q: Default color on scheduler create?

**A:** Auto-assigned: `palette[count(existing schedulers in workspace) % 16]`.
Deterministic order so the first 16 schedulers each get a distinct color.
Picker shows the auto-selected swatch pre-highlighted; user can override.

### Q: Where else is the color shown?

**A:** v1: calendar only. Don't add color to the bindings list page, the
sidebar entry, or the scheduler workspace catalog list — that's UI churn that
should ship in a follow-up after the calendar is validated.

---

## 7. "Calendars" rail (toggle panel)

### Q: Placement?

**A:** **Right side** of the calendar viewport, fixed width 240px, always
visible at desktop widths. (Left side is used by the global Pi Dash sidebar.)
Hidden on mobile (<768px) — mobile only ever renders one scheduler's
occurrences at a time, picked via dropdown if multiple are installed.

### Q: Contents?

**A:** For each scheduler installed on the project (i.e. has at least one
binding):

- Color swatch (clickable to recolor — opens the palette picker if user has
  workspace-admin permission, else read-only)
- Scheduler name
- Checkbox to toggle visibility of its occurrences on the calendar
- Subtitle: `N bindings` if scheduler has multiple bindings on this project
  (unlikely but possible)

A "Show all" / "Hide all" pair at the top.

### Q: State persistence?

**A:** Visibility toggles persist per-user per-project in
`localStorage` under key `scheduler-calendar:visible:<projectId>`. No server
round-trip. Reset to "all visible" if the stored set is empty or all
schedulers have been uninstalled.

### Q: Collapse?

**A:** No collapse in v1. Always-visible 240px panel. If a user has 50
schedulers on one project, the rail scrolls internally.

---

## 8. Density & overlap handling

### Q: Threshold for rolling up high-frequency bindings?

**A:** If a single binding produces > 50 occurrences in the visible date
window (≈ 1.6/day average across a month, or any minute-frequency binding):

- **Month view:** render as a single block per day with label `{N}× <name>`
  (e.g. `3× Security audit`). Click expands a popover listing the firings.
- **Week view:** still render individual blocks (the time axis spreads them
  out enough that 50 in a day is visually OK).

### Q: Overlapping occurrences in week view (multiple at same minute)?

**A:** Side-by-side at proportional widths (Google Calendar style):

- 2 overlapping → each at 50% column width.
- 3 → each at 33%.
- 4+ → render first 3 at 25% width; the rest collapse into a "+N more" stub at
  the right edge of the column slot. Clicking the stub opens a popover list.

### Q: Empty state?

**A:** Project has 0 bindings installed: calendar shows a centered illustration

- message: "No schedulers installed on this project. Switch to the List tab to
  install one." Primary button: "Go to List" → switches tab.

### Q: Loading state?

**A:**

- First load: month-view shows skeleton tiles (gray pulse blocks matching the
  grid layout). Week-view shows skeleton bars sprinkled across day columns.
- Date-range change (forward/back): keep previous data on screen, show a thin
  progress bar at the top. Replace on success. Same UX as the issue calendar.

### Q: Error state?

**A:** Network error fetching occurrences: toast + retry button. Stale data
remains visible so the user isn't dropped into a blank screen.

---

## 9. Drawer (click behavior)

### Q: Click on a future (scheduled) occurrence?

**A:** Opens a side drawer (right side, 480px) showing:

- Scheduler name, color swatch
- "Next firing: {datetime in user tz} ({tzid} time)"
- Read-only RRULE + humanized text
- Binding's extra context (collapsed if long)
- Actions: **Edit binding** (opens existing edit modal), **Disable binding**,
  **View scheduler definition** (opens scheduler page)

### Q: Click on a past occurrence?

**A:** Opens the same drawer template, but content is the `AgentRun` detail:

- Scheduler name, color swatch
- "Fired at: {datetime}, completed at: {datetime} (Xs)"
- Status badge
- Link: "View full agent run →" (opens the existing AgentRun drawer/page if
  one exists — TODO confirm where AgentRun detail lives today, may need a
  separate sub-task in PR3 to wire up)

### Q: Multi-select / range-select?

**A:** Not in v1.

---

## 10. Mobile

### Q: What renders on mobile (<768px)?

**A:** Month view only. Switching to week view via the header toggle shows a
toast: "Week view requires a wider screen." The toggle is still present (so
the user sees the option exists), but the view stays in month mode. Mirrors
the issue calendar's mobile fallback pattern.

### Q: "Calendars" rail on mobile?

**A:** Hidden. Replaced by a dropdown in the header: "Filter: All schedulers"
→ multi-select dropdown.

### Q: Drawer on mobile?

**A:** Full-screen modal instead of side drawer. Already the propel
`<Drawer>` component's default mobile behavior.

---

## 11. Permissions

### Q: Who can view the calendar?

**A:** Same as the bindings list — admin, member, guest. Project scope.

### Q: Who can edit `Scheduler.color`?

**A:** Workspace admin (same gate as the rest of the Scheduler edit form). The
"recolor" action in the calendars rail is hidden / disabled for non-admins.

### Q: Who can edit / disable bindings from the drawer?

**A:** Project admin (same as the existing bindings list). Non-admins see the
drawer in read-only mode.

---

## 12. Drag-to-create / drag-to-reschedule

### Q: Either of these in v1?

**A:** **No to both.** Bindings are created via the existing install modal;
moving a single occurrence is an `EXDATE + RDATE` override, which we've
deferred to a later PR. Week view and month view are read-only with respect
to scheduling.

This keeps PR3's component surface honest: we're not copying the issue
calendar's drag-drop code; we're explicitly omitting it.

---

## 13. Frontend recurrence input (PR1, deferred picker)

### Q: PR1 install/edit modal — what replaces the cron text input?

**A:** Three new form fields, all plain inputs:

1. **dtstart** — datetime picker (use the existing propel `<DateTimePicker>`)
2. **tzid** — searchable `<Select>` populated from `Intl.supportedValuesOf("timeZone")`
3. **rrule** — `<Textarea>` for the raw RRULE string + below it a small grey
   line rendering `rrulestr(rrule, dtstart=dtstart).toText()` via the
   `rrule` npm package (auto-installs on PR1 — already in catalog?
   add if not).

`rdates` and `exdates` are not user-editable in PR1 — they ship as empty
arrays and become editable in the v1.1 "manage occurrences" PR.

### Q: When does the visual recurrence picker (calendar-style "repeat every N

weeks on M") arrive?

**A:** Separate PR, after PR3 ships. Not on the v1 critical path.

---

## 14. List page changes (PR1)

### Q: What changes in the bindings list table (`bindings-panel.tsx`)?

**A:**

- Replace the `cron` column with `Schedule` showing humanized RRULE via
  `rrule.toText()` (e.g. "every weekday at 9 AM").
- Tooltip on the cell shows the raw RRULE string.
- Color swatch shown next to the scheduler name in the first column (the
  scheduler's color, sourced from the joined `Scheduler.color`).

These are the only PR1 UI changes; the calendar tab itself comes in PR3.

---

## 15. Risks & open follow-ups

**Risks called out for PR review:**

- **Cron→RRULE conversion correctness.** The dry-run command output must be
  attached to PR1's description. Reviewer should spot-check at least 5
  bindings from production data.
- **Week-axis component is new code.** No existing primitive to copy. PR3 size
  estimate (~1000-1200 LOC) is biased high but could still surprise.
- **Occurrences endpoint cost at scale.** 100+ bindings × 90-day window ×
  minute-frequency = up to 100k occurrences before the 5000 cap. The cap
  saves us, but the user experience of "+N more" hints could be noisy.

**Follow-ups not in this scope:**

- v1.1: visual recurrence picker (the calendar-style "repeat every N weeks on
  M" UI). Replaces the RRULE textarea.
- v1.2: manage `rdates`/`exdates` in the UI (skip-once / move-once). Requires
  the override-rendering work the calendar is already designed for.
- v1.3: read-only `.ics` feed at
  `/api/workspaces/<slug>/projects/<id>/schedulers.ics` with token-signed URLs
  for Google Calendar / Apple Calendar subscription. Reuses the same RRULE
  data.
- v2: drag-to-reschedule on the week view. Implementation = creating an
  EXDATE for the original time + an RDATE for the new time. Requires
  override-occurrence rendering.
- v2: workspace-level scheduler calendar (overview of all projects' bindings).
- v2: per-user color preference (override the scheduler's default).

---

## 16. PR breakdown summary (reference)

This is a reference, not the source of truth — full PR-by-PR task lists go
into `tasks.md` when implementation starts.

| PR      | LOC est.   | Backend                                                                                                                                                                             | Frontend                                                                                                                                                    |
| ------- | ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **PR1** | ~700       | Schema migration (drop cron, add 5 binding cols + 1 scheduler col); cron→RRULE converter + dry-run command; dateutil swap in `bgtasks/scheduler.py`; serializer/view updates; tests | Install/edit modal (3 new fields, deferred picker); bindings list (humanized schedule column + color swatch)                                                |
| **PR2** | ~300       | Occurrences endpoint with Valkey caching; contract tests                                                                                                                            | —                                                                                                                                                           |
| **PR3** | ~1000-1200 | —                                                                                                                                                                                   | Calendar route + layout with tab bar; month-view (copy-adapt); week-view time axis (new); calendars rail; drawer; occurrences hook; visibility-toggle state |

Each PR is independently mergeable and revertable. PR2 depends on PR1's data
shape; PR3 depends on PR2's endpoint. PR1 depends on PR #153 being merged
into `main` (the schedulers list page surface).
