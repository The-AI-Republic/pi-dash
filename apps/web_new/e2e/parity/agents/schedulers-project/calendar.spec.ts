// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-185): the project firing calendar renders
// occurrences on week and month grids with period stepping and jump-to-today;
// today is marked and a live time line tracks the current time; overcrowded
// days collapse behind overflow controls and capped windows hint at
// truncation; blocks are click-to-inspect.
// Row: AGT-015.
// Note: past occurrences need AgentRun rows, which the seeded stack never
// produces (no beat), so every block below is future-styled; the
// past/future visual distinction half is a Gap.
import { test, expect } from "../../fixtures";
import {
  ROLE,
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverBindings,
  serverCreateBinding,
  serverOccurrences,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatProjectRole } from "../support";

const ROWS = ["AGT-015"];
/** Past-block grey from the shared occurrence styling (none below may match it). */
const PAST_GREY = "rgb(229, 231, 235)";

/** Next Sunday 00:00 local through the Saturday after: the stepped-to week window. */
function nextWeekWindow(): { from: string; to: string } {
  const now = new Date();
  const sunday = new Date(now);
  sunday.setDate(now.getDate() + ((7 - now.getDay()) % 7 || 7));
  sunday.setHours(0, 0, 0, 0);
  return { from: sunday.toISOString(), to: new Date(sunday.getTime() + 7 * 24 * 3600_000 - 1).toISOString() };
}

function monthYearLabel(at: Date): string {
  return new Intl.DateTimeFormat(undefined, { month: "long", year: "numeric" }).format(at);
}

test(
  specTitle(ROWS, "firing calendar renders weeks and months with stepping, marks and overflow"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-ag15"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;

    const projects = await test.step("owner prepares daily, hourly and minutely projects", async () => {
      const out: Record<string, { id: string }> = {};
      const dtstart = new Date(Date.now() - 30 * 24 * 3600_000).toISOString();
      for (const [key, slug, name, rrule, ident] of [
        ["daily", `agt15-daily-${workspaceSlug}`, "AGT15 Daily Definition", "FREQ=DAILY", "AG15"],
        ["hourly", `agt15-hourly-${workspaceSlug}`, "AGT15 Hourly Definition", "FREQ=HOURLY", "AG1H"],
        ["minutely", `agt15-min-${workspaceSlug}`, "AGT15 Minutely Definition", "FREQ=MINUTELY", "AG1N"],
      ]) {
        const created = await ensureProject(
          workspaceSlug,
          ownerSession,
          `AGT15 ${key} ${tag}`,
          parityProjectIdentifier(ident)
        );
        const definition = await ensureScheduler(workspaceSlug, ownerSession, {
          slug,
          name,
          prompt: "Audit this project nightly.",
          is_enabled: true,
        });
        const rows = await serverBindings(workspaceSlug, created.id, ownerSession);
        if (!rows.some((row) => row.scheduler === definition.id)) {
          await serverCreateBinding(workspaceSlug, created.id, ownerSession, {
            scheduler: definition.id,
            project: created.id,
            dtstart,
            rrule,
          });
        }
        out[key] = created;
      }
      return out as Record<"daily" | "hourly" | "minutely", { id: string }>;
    });

    await test.step("owner signs in; the calendar opens on the week view", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectCalendar(workspaceSlug, projects.daily.id);
      expect(await driver.schedulerCalendarView()).toBe("week");
      expect(await driver.schedulerCalendarEmptyVisible()).toBe(false);
    });

    await test.step("week blocks match the API occurrences and read future-styled", async () => {
      await driver.schedulerCalendarStep("next");
      const window = nextWeekWindow();
      const api = await serverOccurrences(workspaceSlug, projects.daily.id, ownerSession, window.from, window.to);
      expect(api.has_more).toBe(false);
      expect(api.occurrences.length).toBe(7);
      await expect.poll(() => driver.schedulerCalendarWeekBlocks(), { timeout: 30_000 }).toHaveLength(7);
      const blocks = await driver.schedulerCalendarWeekBlocks();
      expect(blocks.length).toBe(api.occurrences.length);
      for (const block of blocks) {
        expect(block.name).toBe("AGT15 Daily Definition");
        expect(block.title).toContain("AGT15 Daily Definition");
        expect(block.background).not.toBe(PAST_GREY);
      }
    });

    await test.step("today is marked with a live time line; stepping away clears it", async () => {
      await driver.schedulerCalendarToday();
      expect(await driver.schedulerCalendarWeekTodayMarked()).toBe(true);
      const top = await driver.schedulerCalendarTimeLineTop();
      expect(top).not.toBeNull();
      // The line ticks every minute, so it may lag the read by up to a
      // minute (including across an hour boundary); either endpoint proves
      // it tracks the current time rather than sitting static.
      const candidates = [Date.now(), Date.now() - 60_000].map((stamp) => {
        const at = new Date(stamp);
        return ((at.getHours() * 60 + at.getMinutes()) / 60) * 48;
      });
      const skew = Math.min(...candidates.map((expected) => Math.abs((top ?? -1) - expected)));
      expect(skew).toBeLessThan(5);
      await driver.schedulerCalendarStep("next");
      await driver.schedulerCalendarStep("next");
      expect(await driver.schedulerCalendarTimeLineTop()).toBeNull();
      expect(await driver.schedulerCalendarWeekTodayMarked()).toBe(false);
      await driver.schedulerCalendarToday();
      expect(await driver.schedulerCalendarWeekTodayMarked()).toBe(true);
      expect(await driver.schedulerCalendarTitle()).toBe(monthYearLabel(new Date()));
    });

    await test.step("month grids render occurrences with today marked and stepping", async () => {
      await driver.schedulerCalendarSetView("month");
      expect(await driver.schedulerCalendarView()).toBe("month");
      await expect.poll(() => driver.schedulerCalendarMonthBlocks(), { timeout: 30_000 }).not.toEqual([]);
      expect(await driver.schedulerCalendarMonthTodayMarked()).toBe(true);
      const blocks = await driver.schedulerCalendarMonthBlocks();
      expect(blocks.length).toBeGreaterThan(0);
      for (const block of blocks) {
        expect(block.name).toBe("AGT15 Daily Definition");
      }
      const now = new Date();
      const nextMonth = new Date(now.getFullYear(), now.getMonth() + 1, 1);
      await driver.schedulerCalendarStep("next");
      expect(await driver.schedulerCalendarTitle()).toBe(monthYearLabel(nextMonth));
      expect(await driver.schedulerCalendarMonthTodayMarked()).toBe(false);
      await expect.poll(() => driver.schedulerCalendarMonthBlocks(), { timeout: 30_000 }).not.toEqual([]);
      await driver.schedulerCalendarToday();
      expect(await driver.schedulerCalendarTitle()).toBe(monthYearLabel(now));
    });

    await test.step("overcrowded days collapse behind an overflow control", async () => {
      await driver.schedulerOpenProjectCalendar(workspaceSlug, projects.hourly.id);
      await driver.schedulerCalendarSetView("month");
      await driver.schedulerCalendarStep("next");
      // Full future days hold 24 hourly firings: 4 blocks plus the control.
      await expect.poll(() => driver.schedulerCalendarMonthOverflow(), { timeout: 30_000 }).toContain("+ 20 more");
      expect(await driver.schedulerCalendarTruncatedVisible()).toBe(false);
    });

    await test.step("capped windows hint at truncation with a density rollup", async () => {
      const now = Date.now();
      const api = await serverOccurrences(
        workspaceSlug,
        projects.minutely.id,
        ownerSession,
        new Date(now - 7 * 24 * 3600_000).toISOString(),
        new Date(now + 7 * 24 * 3600_000).toISOString()
      );
      expect(api.has_more).toBe(true);
      expect(api.next_window_start).not.toBeNull();
      await driver.schedulerOpenProjectCalendar(workspaceSlug, projects.minutely.id);
      await driver.schedulerCalendarStep("next");
      await expect.poll(() => driver.schedulerCalendarTruncatedVisible(), { timeout: 30_000 }).toBe(true);
      await driver.schedulerCalendarSetView("month");
      await driver.schedulerCalendarStep("next");
      await expect.poll(() => driver.schedulerCalendarTruncatedVisible(), { timeout: 30_000 }).toBe(true);
      await expect
        .poll(
          async () =>
            (await driver.schedulerCalendarMonthOverflow()).some((text) =>
              /^\d+× AGT15 Minutely Definition$/.test(text)
            ),
          { timeout: 30_000 }
        )
        .toBe(true);
    });

    await test.step("clicking a block inspects its occurrence", async () => {
      await driver.schedulerOpenProjectCalendar(workspaceSlug, projects.daily.id);
      await driver.schedulerCalendarStep("next");
      await expect.poll(() => driver.schedulerCalendarWeekBlocks(), { timeout: 30_000 }).toHaveLength(7);
      await driver.schedulerCalendarClickBlock("AGT15 Daily Definition");
      const heading = await driver.schedulerDrawerHeading();
      expect(heading.name).toBe("AGT15 Daily Definition");
      expect(heading.state).toBe("Scheduled");
      await driver.schedulerDrawerClose();
      expect(await driver.schedulerDrawerOpen()).toBe(false);
    });

    await test.step("a project member sees the same blocks", async () => {
      const member = await seatProjectRole(harness, projects.daily.id, ROLE.MEMBER, ROLE.MEMBER, "parity-ag15m");
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.schedulerOpenProjectCalendar(workspaceSlug, projects.daily.id);
      await driver.schedulerCalendarStep("next");
      await expect.poll(() => driver.schedulerCalendarWeekBlocks(), { timeout: 30_000 }).toHaveLength(7);
    });
  }
);
