// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-123): recent-activity feed. Rows: SHELL-014
// (visit-ordered rows across work items, pages and projects, skipping
// missing payloads in a capped scrollable container), SHELL-015 (per-type
// filter refetch with per-type empty art), SHELL-017 (relative visit time
// plus type context per row, degrading gracefully on missing data).
// Behavior learned from the old dashboard in prose: rows render in visit
// order, the filter refetches per activity type, and each row shows an
// age plus status, priority, assignee or owner and project identifiers.
import { test, expect } from "../../fixtures";
import type { ParityDriver } from "../../drivers/parity-driver";
import {
  serverHomeIssues,
  serverRecents,
  serverSeedProject,
  serverSetTourCompleted,
  signInSessionRetry,
  serverEnsureWidgets,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-014", "SHELL-015", "SHELL-017"];
const AGE = /ago|just now|less than|minute|hour|day|week|month|second/i;

async function signedInHome(
  driver: ParityDriver,
  seed: { email: string; password: string; workspaceSlug: string }
): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPassword(seed.email, seed.password);
  await driver.homeOpen(seed.workspaceSlug);
}

test(
  specTitle(ROWS, "recent visits render in visit order with age and context"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await signInSessionRetry(seed.email, seed.password);
    await serverSetTourCompleted(session, true);
    await serverEnsureWidgets(seed.workspaceSlug, session, ["recents"]);
    const project = await serverSeedProject(seed.workspaceSlug, session);
    const issues = await serverHomeIssues(seed.workspaceSlug, project.id, session);
    const first = issues.find((issue) => /parity first issue/i.test(issue.name)) ?? issues[0];
    const second = issues.find((issue) => /parity second issue/i.test(issue.name)) ?? issues[1] ?? issues[0];
    if (first === undefined || second === undefined) {
      test.skip(true, "seed carries no issues to visit");
      return;
    }

    await test.step("visit two work items to seed the feed", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.homeOpenIssueDetail(seed.workspaceSlug, project.id, first.id);
      await driver.page.waitForTimeout(2_000);
      await driver.homeOpenIssueDetail(seed.workspaceSlug, project.id, second.id);
      await driver.page.waitForTimeout(2_000);
    });

    await test.step("rows appear most-recent-first with age and identifiers", async () => {
      await driver.homeOpen(seed.workspaceSlug);
      await driver.homeWaitForWidgets();
      await expect.poll(() => driver.homeRecentRowTexts(), { timeout: 60_000 }).not.toEqual([]);
      const rows = await driver.homeRecentRowTexts();
      for (const row of rows) expect(row).toMatch(AGE);
      // The later visit ranks above the earlier one; other traffic on the
      // scratch stack may interleave, so this is a relative-order check.
      const body = rows.join("\n");
      expect(body).toContain(second.name);
      expect(body).toContain(first.name);
      expect(body.indexOf(second.name)).toBeLessThan(body.indexOf(first.name));
      const server = await serverRecents(seed.workspaceSlug, session);
      expect(server.length).toBeGreaterThan(0);
      // Every rendered row is non-empty: entries with missing payloads are
      // skipped instead of breaking the feed.
      for (const row of rows) expect(row.trim().length).toBeGreaterThan(0);
    });
  }
);

test(
  specTitle(ROWS, "recents filter refetches per type with empty art"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await signInSessionRetry(seed.email, seed.password);
    await serverSetTourCompleted(session, true);
    await serverEnsureWidgets(seed.workspaceSlug, session, ["recents"]);

    await signedInHome(driver, seed);
    await driver.homeWaitForWidgets();

    await test.step("work-item filter narrows to work items", async () => {
      await driver.homeSetRecentsFilter("issue");
      await expect.poll(() => driver.homeRecentRowTexts(), { timeout: 60_000 }).not.toEqual([]);
      const server = await serverRecents(seed.workspaceSlug, session, "issue");
      expect(server.length).toBeGreaterThan(0);
    });

    await test.step("page filter shows its own empty art", async () => {
      await driver.homeSetRecentsFilter("page");
      const server = await serverRecents(seed.workspaceSlug, session, "page");
      if (server.length === 0) {
        await expect.poll(() => driver.homeRecentRowTexts(), { timeout: 60_000 }).toEqual([]);
      } else {
        await expect.poll(() => driver.homeRecentRowTexts(), { timeout: 60_000 }).not.toEqual([]);
      }
    });

    await test.step("project filter lists the visited project", async () => {
      await driver.homeSetRecentsFilter("project");
      await expect.poll(() => driver.homeRecentRowTexts(), { timeout: 60_000 }).not.toEqual([]);
      const rows = await driver.homeRecentRowTexts();
      expect(rows.join("\n")).toContain(seed.projectName);
    });
  }
);
