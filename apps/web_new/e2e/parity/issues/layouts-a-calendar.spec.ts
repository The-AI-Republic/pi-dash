// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-117): the calendar layout — month/week views
// (ISS-021), navigation (ISS-022), options (ISS-023), drag to re-date
// (ISS-024), issue blocks (ISS-025), and per-day add (ISS-026).
import { test, expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import {
  seedProjectUserProperties,
  serverCreateIssue,
  serverDeleteIssue,
  serverIssueDetails,
  serverIssues,
  serverPatchIssue,
  serverPatchProjectUserProperties,
  serverProjectUserProperties,
  sessionBrowserCookies,
  signInSession,
  uniqueSuffix,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test.use({ viewport: { width: 1600, height: 900 } });

interface CalDay {
  day: number;
  iso: string;
}

function currentMonthDays(): { today: CalDay; others: CalDay[] } {
  // Day numbers strictly inside the current month: the grid also renders
  // adjacent-month filler tiles, so fixtures never touch day 1 or the
  // last day, and weekend visibility never hides them (specs enable it).
  const now = new Date();
  const last = new Date(now.getFullYear(), now.getMonth() + 1, 0).getDate();
  const iso = (day: number): string =>
    `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
  const inMonth = (day: number): boolean => day >= 2 && day <= last - 1;
  const today: CalDay = { day: now.getDate(), iso: iso(now.getDate()) };
  const others: CalDay[] = [];
  for (const candidate of [3, 5, 8, 12, 15, 18, 22, 25]) {
    if (candidate === today.day || !inMonth(candidate)) continue;
    const before = candidate - 1;
    const after = candidate + 1;
    if (inMonth(before) && !others.some((d) => d.day === before)) others.push({ day: before, iso: iso(before) });
    others.push({ day: candidate, iso: iso(candidate) });
    if (inMonth(after) && !others.some((d) => d.day === after)) others.push({ day: after, iso: iso(after) });
    if (others.length >= 6) break;
  }
  return { today, others: others.slice(0, 6) };
}

async function resetPrefs(workspaceSlug: string, projectId: string, session: string): Promise<void> {
  const seed = seedProjectUserProperties();
  await serverPatchProjectUserProperties(workspaceSlug, projectId, session, {
    display_filters: seed.display_filters,
    display_properties: seed.display_properties,
  });
}

async function openCal(
  driver: Pick<ParityDriver, "openAuthenticated">,
  seed: Pick<ParitySeedFacts, "workspaceSlug" | "projectId">,
  session: string
): Promise<void> {
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
    display_filters: {
      layout: "calendar",
      group_by: null,
      order_by: "sort_order",
      calendar: { layout: "month", show_weekends: true },
    },
  });
  await driver.openAuthenticated(
    `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
    sessionBrowserCookies(session)
  );
}

test(
  specTitle(["ISS-021"], "month view buckets dated issues onto day tiles"),
  { tag: specTags(["ISS-021"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const { today, others } = currentMonthDays();
    const [dayA, dayB] = [others[0] ?? today, others[1] ?? today];
    const suffix = uniqueSuffix();
    const nameA = `Parity cal A ${suffix}`;
    const nameB = `Parity cal B ${suffix}`;
    const idA = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, nameA);
    const idB = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, nameB);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, idA, { target_date: dayA.iso }, session);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, idB, { target_date: dayB.iso }, session);
    try {
      await openCal(driver, seed, session);
      expect(await driver.layoutsCalendarVisible()).toEqual(true);
      expect(await driver.layoutsCalMode()).toEqual("month");
      expect(await driver.layoutsCalColumnCount()).toEqual(7);

      await test.step("each dated issue lands on its day", async () => {
        await expect.poll(async () => driver.layoutsCalDayIssueNames(dayA.day), { timeout: 300_000 }).toContain(nameA);
        await expect.poll(async () => driver.layoutsCalDayIssueNames(dayB.day), { timeout: 300_000 }).toContain(nameB);
      });

      await test.step("today is badged and undated issues never appear", async () => {
        expect(await driver.layoutsCalDayIsToday(today.day)).toEqual(true);
        for (const tile of [dayA.day, dayB.day, today.day]) {
          const names = await driver.layoutsCalDayIssueNames(tile);
          for (const undated of seed.issueNames) expect(names).not.toContain(undated);
        }
      });
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, idA, session);
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, idB, session);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-021"], "crowded days page through load more"),
  { tag: specTags(["ISS-021"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const { others } = currentMonthDays();
    const crowded = others[2] ?? others[0] ?? { day: 15, iso: "" };
    const suffix = uniqueSuffix();
    const names = Array.from({ length: 6 }, (_, k) => `Parity crowd ${k} ${suffix}`);
    const ids: string[] = [];
    try {
      for (const name of names) {
        const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, name);
        await serverPatchIssue(seed.workspaceSlug, seed.projectId, id, { target_date: crowded.iso }, session);
        ids.push(id);
      }
      await openCal(driver, seed, session);
      await expect.poll(async () => driver.layoutsCalDayHasLoadMore(crowded.day), { timeout: 300_000 }).toEqual(true);
      await driver.layoutsCalDayLoadMore(crowded.day);
      await expect
        .poll(async () => driver.layoutsCalDayIssueNames(crowded.day), { timeout: 300_000 })
        .toEqual(expect.arrayContaining(names));
    } finally {
      for (const id of ids) await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-022"], "step months, jump to today, pick from the month picker"),
  { tag: specTags(["ISS-022"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const { today, others } = currentMonthDays();
    const home = others[3] ?? others[0] ?? today;
    const suffix = uniqueSuffix();
    const name = `Parity calnav ${suffix}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, name);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, id, { target_date: home.iso }, session);
    try {
      await openCal(driver, seed, session);
      const title = await driver.layoutsCalTitle();
      await expect.poll(async () => driver.layoutsCalDayIssueNames(home.day), { timeout: 300_000 }).toContain(name);

      await test.step("prev/next move one month and refetch the window", async () => {
        await driver.layoutsCalPrev();
        const prevTitle = await driver.layoutsCalTitle();
        expect(prevTitle).not.toEqual(title);
        expect(await driver.layoutsCalDayIssueNames(home.day)).not.toContain(name);
        await driver.layoutsCalNext();
        await expect.poll(async () => driver.layoutsCalTitle(), { timeout: 300_000 }).toEqual(title);
        await expect.poll(async () => driver.layoutsCalDayIssueNames(home.day), { timeout: 300_000 }).toContain(name);
      });

      await test.step("today jumps back to the current month", async () => {
        await driver.layoutsCalNext();
        expect(await driver.layoutsCalTitle()).not.toEqual(title);
        await driver.layoutsCalToday();
        await expect.poll(async () => driver.layoutsCalTitle(), { timeout: 300_000 }).toEqual(title);
        expect(await driver.layoutsCalDayIsToday(today.day)).toEqual(true);
      });

      await test.step("the title picker offers months with year stepping", async () => {
        expect(await driver.layoutsCalMonthPickerEnabled()).toEqual(true);
        // The picker grid shows short titles ("Jan"); the title bar shows
        // the full month name.
        const months = await driver.layoutsCalMonthPickerMonths();
        expect(months).toHaveLength(12);
        expect(months).toEqual(expect.arrayContaining(["Jan", "Dec"]));
        const year = new Date().getFullYear();
        expect(await driver.layoutsCalMonthPickerYear()).toEqual(year);
        await driver.layoutsCalMonthPickerYearStep("next");
        expect(await driver.layoutsCalMonthPickerYear()).toEqual(year + 1);
        await driver.layoutsCalMonthPickerYearStep("prev");
        expect(await driver.layoutsCalMonthPickerYear()).toEqual(year);
        const currentShort = months[new Date().getMonth()] ?? "";
        const other = months.find((m) => m !== currentShort && m !== "") ?? "";
        await driver.layoutsCalMonthPickerChoose(other);
        await expect.poll(async () => driver.layoutsCalTitle(), { timeout: 300_000 }).not.toEqual(title);
        await driver.layoutsCalToday();
        await expect.poll(async () => driver.layoutsCalTitle(), { timeout: 300_000 }).toEqual(title);
      });
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-022"], "week view shows a range title and disables the picker"),
  { tag: specTags(["ISS-022"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    try {
      await openCal(driver, seed, session);
      await driver.layoutsCalSetMode("week");
      expect(await driver.layoutsCalMode()).toEqual("week");
      const weekTitle = await driver.layoutsCalTitle();
      // A same-month week shares the month title ("October 2026"), so the
      // inequality below cannot assume they differ; the picker being
      // disabled plus the prev/today round-trip proves week mode instead.
      expect(weekTitle).toContain(String(new Date().getFullYear()));
      expect(await driver.layoutsCalMonthPickerEnabled()).toEqual(false);
      let stepped = weekTitle;
      for (let k = 0; k < 6 && stepped === weekTitle; k++) {
        await driver.layoutsCalPrev();
        stepped = await driver.layoutsCalTitle();
      }
      expect(stepped).not.toEqual(weekTitle);
      await driver.layoutsCalToday();
      await expect.poll(async () => driver.layoutsCalTitle(), { timeout: 300_000 }).toEqual(weekTitle);
    } finally {
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-023"], "options switch month/week and toggle weekends"),
  { tag: specTags(["ISS-023"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    try {
      await openCal(driver, seed, session);
      expect(await driver.layoutsCalMode()).toEqual("month");

      await driver.layoutsCalSetMode("week");
      expect(await driver.layoutsCalMode()).toEqual("week");
      await driver.layoutsCalSetMode("month");
      expect(await driver.layoutsCalMode()).toEqual("month");

      await driver.layoutsCalSetWeekends(false);
      expect(await driver.layoutsCalWeekendsVisible()).toEqual(false);
      expect(await driver.layoutsCalColumnCount()).toEqual(5);
      await driver.layoutsCalSetWeekends(true);
      expect(await driver.layoutsCalWeekendsVisible()).toEqual(true);
      expect(await driver.layoutsCalColumnCount()).toEqual(7);

      const stored = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, session);
      const calendar = stored.displayFilters["calendar"] as Record<string, unknown>;
      expect(calendar["layout"]).toEqual("month");
      expect(calendar["show_weekends"]).toEqual(true);
    } finally {
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-024"], "drag a block to another day re-dates it"),
  { tag: specTags(["ISS-024"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const { others } = currentMonthDays();
    const from = others[0] ?? { day: 8, iso: "" };
    const to = others[1] ?? { day: 9, iso: "" };
    const suffix = uniqueSuffix();
    const name = `Parity drag ${suffix}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, name);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, id, { target_date: from.iso }, session);
    try {
      await openCal(driver, seed, session);
      await expect.poll(async () => driver.layoutsCalDayIssueNames(from.day), { timeout: 300_000 }).toContain(name);

      await driver.layoutsCalDragBlock(name, to.day);
      await expect.poll(async () => driver.layoutsCalDayIssueNames(to.day), { timeout: 300_000 }).toContain(name);
      expect(await driver.layoutsCalDayIssueNames(from.day)).not.toContain(name);
      expect((await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, session)).targetDate).toEqual(to.iso);
      expect(await driver.layoutsRowHighlighted(name)).toEqual(true);

      await test.step("dropping onto the same day is a no-op", async () => {
        await driver.layoutsCalDragBlock(name, to.day);
        expect((await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, session)).targetDate).toEqual(to.iso);
      });
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-024"], "a drop before the start date is rejected with a toast"),
  { tag: specTags(["ISS-024"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const { others } = currentMonthDays();
    // Start late in the sampled days, dated on the start, dropped earlier.
    const days = [...others].sort((a, b) => a.day - b.day);
    const late = days[days.length - 1] ?? { day: 20, iso: "" };
    const early = days[0] ?? { day: 8, iso: "" };
    const suffix = uniqueSuffix();
    const name = `Parity dragrej ${suffix}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, name);
    await serverPatchIssue(
      seed.workspaceSlug,
      seed.projectId,
      id,
      { target_date: late.iso, start_date: late.iso },
      session
    );
    try {
      await openCal(driver, seed, session);
      await expect.poll(async () => driver.layoutsCalDayIssueNames(late.day), { timeout: 300_000 }).toContain(name);
      await driver.layoutsCalDragBlock(name, early.day);
      const toast = await driver.rulesLastToast();
      expect(toast).not.toBeNull();
      expect((await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, session)).targetDate).toEqual(late.iso);
      await expect.poll(async () => driver.layoutsCalDayIssueNames(late.day), { timeout: 300_000 }).toContain(name);
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-025"], "blocks preview on hover, open peek, and offer quick actions"),
  { tag: specTags(["ISS-025"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const { others } = currentMonthDays();
    const home = others[4] ?? others[0] ?? { day: 12, iso: "" };
    const suffix = uniqueSuffix();
    const name = `Parity block ${suffix}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, name);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, id, { target_date: home.iso }, session);
    try {
      await openCal(driver, seed, session);
      await expect.poll(async () => driver.layoutsCalDayIssueNames(home.day), { timeout: 300_000 }).toContain(name);

      const text = await driver.layoutsCalBlockText(name);
      expect(text).toContain(name);
      expect(text).toMatch(/[A-Z]+-\d+/);

      expect(await driver.layoutsCalBlockHoverPreview(name)).toEqual(true);

      const actions = await driver.layoutsCalBlockQuickActions(name);
      expect(actions).toEqual(expect.arrayContaining(["Edit", "Delete"]));

      await driver.layoutsCalBlockOpenPeek(name);
      expect(await driver.layoutsPeekVisible()).toEqual(true);
      expect(await driver.layoutsPeekTitle()).toEqual(name);
      await driver.layoutsPeekClose();
      expect(await driver.layoutsPeekVisible()).toEqual(false);
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-026"], "add a work item to a day inline"),
  { tag: specTags(["ISS-026"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const { others } = currentMonthDays();
    const home = others[5] ?? others[0] ?? { day: 16, iso: "" };
    try {
      await openCal(driver, seed, session);
      expect(await driver.layoutsCalDayAddMenu(home.day)).toEqual(["Add work item", "Add existing work item"]);

      const title = `Parity caladd ${uniqueSuffix()}`;
      await driver.layoutsCalDayQuickAdd(home.day, title);
      await expect.poll(async () => driver.layoutsCalDayIssueNames(home.day), { timeout: 300_000 }).toContain(title);
      const rows = await serverIssueDetailsForName(seed, session, title);
      expect(rows.targetDate).toEqual(home.iso);
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, rows.id, session);
    } finally {
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-026"], "attach an existing work item to a day"),
  { tag: specTags(["ISS-026"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const { others } = currentMonthDays();
    const home = others[1] ?? others[0] ?? { day: 9, iso: "" };
    const suffix = uniqueSuffix();
    // An issue starting after the target day is filtered out of the modal.
    const lateName = `Parity late ${suffix}`;
    const lateId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, lateName);
    const after = new Date(`${home.iso}T12:00:00Z`);
    after.setUTCDate(after.getUTCDate() + 3);
    await serverPatchIssue(
      seed.workspaceSlug,
      seed.projectId,
      lateId,
      { start_date: after.toISOString().slice(0, 10) },
      session
    );
    const plainName = `Parity plain ${suffix}`;
    const plainId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, plainName);
    try {
      await openCal(driver, seed, session);
      await driver.layoutsCalDayAddExisting(home.day);
      expect(await driver.layoutsAddExistingModalVisible()).toEqual(true);
      await expect
        .poll(async () => driver.layoutsAddExistingModalIssueNames(), { timeout: 300_000 })
        .toEqual(expect.arrayContaining([plainName]));
      expect(await driver.layoutsAddExistingModalIssueNames()).not.toContain(lateName);
      await driver.layoutsAddExistingModalChoose(plainName);
      expect(await driver.layoutsAddExistingModalVisible()).toEqual(false);
      await expect
        .poll(async () => driver.layoutsCalDayIssueNames(home.day), { timeout: 300_000 })
        .toContain(plainName);
      expect((await serverIssueDetails(seed.workspaceSlug, seed.projectId, plainId, session)).targetDate).toEqual(
        home.iso
      );
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, plainId, session);
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, lateId, session);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

async function serverIssueDetailsForName(
  seed: Pick<ParitySeedFacts, "workspaceSlug" | "projectId">,
  session: string,
  name: string
): Promise<{ id: string; targetDate: string | null }> {
  const rows = await serverIssues(seed.workspaceSlug, seed.projectId, session);
  const id = rows.find((row) => row.name === name)?.id ?? "";
  if (!id) throw new Error(`[parity] no server issue named "${name}".`);
  const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, session);
  return { id, targetDate: details.targetDate };
}
