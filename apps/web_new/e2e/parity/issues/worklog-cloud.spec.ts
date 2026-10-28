// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud scenarios (NEWFRONT-672): worklog surfaces on a cloud-flagged
// build. Row: ISS-206 (worklog cloud leg). Edition-gated proof per
// parity/specs/issues.md §23: the OSS leg lives in
// worklog-absent.spec.ts, this file proves the cloud replacement. It
// runs on the "new" project only — the OSS oracle provably lacks the
// surface and the private cloud oracle's UI is intentionally not pinned
// here. The seed user is a workspace admin, hence always eligible to
// log; the member/guest edges stay unit-covered in eligibility.test.ts.
import { test, expect } from "../fixtures";
import { serverCreateIssueFull, serverCleanupIssueWithSession, signInSession } from "../helpers/api";
import { browserCookiesFromHeader } from "../helpers/cloud-session";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-206"], "cloud: log time and see it in the feed and sidebar"),
  { tag: specTags(["ISS-206"]) },
  async ({ driver, seed, page, context }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud worklog pins the web_new UI only");
    const tag = `NF672 worklog ${Date.now()}`;
    const note = `${tag} note`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      // The cloud /login is OIDC-only: act as the seed user through API
      // session cookies instead of the UI sign-in (NEWFRONT-984).
      await context.addCookies(browserCookiesFromHeader(session));
      await driver.openIssueDetail(seed.workspaceSlug, seed.projectId, issue.id);

      await test.step("log-time control renders in the activity header", async () => {
        await expect(page.getByRole("button", { name: "Log time" })).toBeVisible({ timeout: 30_000 });
      });

      await test.step("a logged entry lands in the feed", async () => {
        await driver.worklogLogEntry(90, "2026-10-20", note);
        await expect
          .poll(async () => (await driver.activityEntryTexts()).join("\n"), { timeout: 30_000 })
          .toContain("logged 1h 30m.");
      });

      await test.step("the sidebar totals the entry", async () => {
        await expect.poll(() => driver.worklogSidebarTotal(), { timeout: 30_000 }).toBe("1h 30m");
        await expect
          .poll(() => driver.worklogSidebarEntryTexts(), { timeout: 30_000 })
          .toEqual([expect.stringContaining(note)]);
      });

      await test.step("the worklog filter category narrows the feed", async () => {
        const options = await driver.activityFilterOptions();
        expect(options.map((option) => option.label)).toEqual(["Updates", "Comments", "State", "Assignee", "Worklog"]);
        await driver.activityToggleFilter("Worklog");
        await expect
          .poll(async () => (await driver.activityEntryTexts()).join("\n"), { timeout: 30_000 })
          .not.toContain("logged 1h 30m.");
        await driver.activityToggleFilter("Worklog");
        await expect
          .poll(async () => (await driver.activityEntryTexts()).join("\n"), { timeout: 30_000 })
          .toContain("logged 1h 30m.");
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);
