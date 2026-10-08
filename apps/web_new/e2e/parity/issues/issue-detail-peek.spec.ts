// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: issue peek panel (NEWFRONT-121, Part B). Opening the
// peek from the list, display modes, URL sync, close paths, header actions,
// loading and error states, and the Properties section — against
// scenario-owned issues, green on apps/web first.
// Rows: ISS-165, ISS-166, ISS-167, ISS-168, ISS-169, ISS-170, ISS-171,
// ISS-172.
import { test, expect } from "../fixtures";
import { clearProjectListFilters, fetchIssue, patchIssue, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { dropIssue, ownIssue, signIn } from "./detail-support";

test(
  specTitle(["ISS-165", "ISS-167"], "open a work item in the peek from the list and by deep link"),
  { tag: specTags(["ISS-165", "ISS-167"]) },
  async ({ driver, seed }) => {
    // Same sibling-filter exposure as the URL-sync test below.
    test.setTimeout(300_000);
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle peek open ${Date.now()}`);
    try {
      await test.step("clicking the list row opens the peek", async () => {
        // Sibling filter specs share the oracle user; a leftover persisted
        // filter hides every row, so start from cleared filters.
        await clearProjectListFilters(seed.workspaceSlug, seed.projectId, session);
        await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
        try {
          await expect.poll(() => driver.visibleIssueNames(), { timeout: 45_000 }).toContain(issue.name);
        } catch {
          // Cold dev-server compile can leave the first list paint empty, or
          // a sibling re-polluted the filters mid-poll; clear once more and
          // reload round-trips it without masking a real regression.
          await clearProjectListFilters(seed.workspaceSlug, seed.projectId, session);
          await driver.page.reload();
          await expect.poll(() => driver.visibleIssueNames(), { timeout: 60_000 }).toContain(issue.name);
        }
        await driver.clickListRow(issue.name);
        await expect.poll(() => driver.peekTitle(), { timeout: 60_000 }).toBe(issue.name);
        expect(await driver.peekIdentifier()).toBe(issue.seq);
        expect(driver.page.url()).toContain(`peekIssueId=${issue.id}`);
      });
      await test.step("a deep link auto-opens the same peek", async () => {
        await driver.openPeek(seed.workspaceSlug, seed.projectId, issue.id);
        await expect.poll(() => driver.peekTitle(), { timeout: 60_000 }).toBe(issue.name);
        expect(await driver.peekIdentifier()).toBe(issue.seq);
      });
      await test.step("the server agrees with the peek", async () => {
        const record = await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(record["name"]).toBe(issue.name);
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-165"], "bug: NEWFRONT-139 nested sub-issue peek needs widget rows"),
  { tag: specTags(["ISS-165"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const parent = await ownIssue(seed, session, `Oracle peek parent ${ts}`);
    const child = await ownIssue(seed, session, `Oracle peek child ${ts}`);
    try {
      await patchIssue(seed.workspaceSlug, seed.projectId, child.id, session, { parent_id: parent.id });
      await driver.openPeek(seed.workspaceSlug, seed.projectId, parent.id);
      await expect.poll(() => driver.peekTitle(), { timeout: 60_000 }).toBe(parent.name);
      await driver.openWidgetSection("Sub-work items");
      await test.step("the peek widget counts the child", async () => {
        await expect.poll(() => driver.widgetProgress("Sub-work items"), { timeout: 30_000 }).toBe("0/1 Done");
      });
      await test.step("bug: no child row renders to nest the peek from", async () => {
        // Intended: clicking the child row nests the peek (peekNestingLevel
        // in the URL, child title on top). Unreachable while rows don't render.
        expect(await driver.widgetRowNames("Sub-work items")).toEqual([]);
        expect((await fetchIssue(seed.workspaceSlug, seed.projectId, child.id, session))["parent_id"]).toBe(parent.id);
      });
    } finally {
      await dropIssue(seed, session, child.id);
      await dropIssue(seed, session, parent.id);
    }
  }
);

test(
  specTitle(["ISS-166"], "peek display modes reshape the panel and reset on reopen"),
  { tag: specTags(["ISS-166"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle peek modes ${Date.now()}`);
    try {
      await driver.openPeek(seed.workspaceSlug, seed.projectId, issue.id);
      await expect.poll(() => driver.peekTitle(), { timeout: 60_000 }).toBe(issue.name);
      const viewport = driver.page.viewportSize() ?? { width: 1280, height: 720 };
      const side = await driver.peekPanelBox();
      expect(side, "side-peek box").not.toBeNull();
      expect(side!.x).toBeGreaterThan(0);
      expect(side!.width).toBeLessThan(viewport.width);
      await test.step("full-screen fills the viewport", async () => {
        await driver.setPeekMode("Full Screen");
        const full = await driver.peekPanelBox();
        expect(full, "full-screen box").not.toBeNull();
        expect(full!.width).toBeGreaterThanOrEqual(side!.width);
        expect(full!.x).toBeLessThanOrEqual(side!.x);
      });
      await test.step("modal sits between the two", async () => {
        await driver.setPeekMode("Modal");
        const modal = await driver.peekPanelBox();
        expect(modal, "modal box").not.toBeNull();
        expect(modal!.width).toBeLessThanOrEqual(viewport.width);
      });
      await test.step("reopening resets to the default dock", async () => {
        await driver.closePeek();
        await driver.openPeek(seed.workspaceSlug, seed.projectId, issue.id);
        await expect.poll(() => driver.peekTitle(), { timeout: 60_000 }).toBe(issue.name);
        const again = await driver.peekPanelBox();
        expect(again, "reopened box").not.toBeNull();
        expect(Math.abs(again!.x - side!.x)).toBeLessThan(120);
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-167"], "peek URL sync omits the route project and back skips the peek"),
  { tag: specTags(["ISS-167"]) },
  async ({ driver, seed }) => {
    // A sibling filter spec can re-pollute user-properties mid-test, so the
    // row poll gets a clear-and-reload retry arm and roomier budget.
    test.setTimeout(300_000);
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const first = await ownIssue(seed, session, `Oracle peek url A ${Date.now()}`);
    const second = await ownIssue(seed, session, `Oracle peek url B ${Date.now()}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, first.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 60_000 }).toBe(first.name);
      await test.step("opening the peek from the list replaces instead of pushing", async () => {
        await clearProjectListFilters(seed.workspaceSlug, seed.projectId, session);
        await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
        try {
          await expect.poll(() => driver.visibleIssueNames(), { timeout: 60_000 }).toContain(second.name);
        } catch {
          await clearProjectListFilters(seed.workspaceSlug, seed.projectId, session);
          await driver.page.reload();
          await expect.poll(() => driver.visibleIssueNames(), { timeout: 60_000 }).toContain(second.name);
        }
        await driver.clickListRow(second.name);
        await expect.poll(() => driver.peekTitle(), { timeout: 60_000 }).toBe(second.name);
        const url = driver.page.url();
        expect(url).toContain(`peekIssueId=${second.id}`);
        expect(url).not.toContain("peekProjectId");
      });
      await test.step("back leaves the list for the previous page", async () => {
        await driver.page.goBack();
        await expect.poll(() => driver.issueDetailTitle(), { timeout: 60_000 }).toBe(first.name);
      });
    } finally {
      await dropIssue(seed, session, second.id);
      await dropIssue(seed, session, first.id);
    }
  }
);

test(
  specTitle(["ISS-168"], "close the peek by button and by escape"),
  { tag: specTags(["ISS-168"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle peek close ${Date.now()}`);
    try {
      await driver.openPeek(seed.workspaceSlug, seed.projectId, issue.id);
      await expect.poll(() => driver.peekTitle(), { timeout: 60_000 }).toBe(issue.name);
      await test.step("header close clears the panel and the param", async () => {
        await driver.closePeek();
        await expect.poll(() => driver.peekOpen(), { timeout: 30_000 }).toBe(false);
        expect(driver.page.url()).not.toContain("peekIssueId");
      });
      await test.step("escape closes the reopened peek", async () => {
        await driver.openPeek(seed.workspaceSlug, seed.projectId, issue.id);
        await expect.poll(() => driver.peekTitle(), { timeout: 60_000 }).toBe(issue.name);
        await driver.page.keyboard.press("Escape");
        await expect.poll(() => driver.peekOpen(), { timeout: 30_000 }).toBe(false);
        expect(driver.page.url()).not.toContain("peekIssueId");
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-169"], "peek header actions copy, fullscreen, and quick actions"),
  { tag: specTags(["ISS-169"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle peek actions ${Date.now()}`);
    try {
      await driver.openPeek(seed.workspaceSlug, seed.projectId, issue.id);
      await expect.poll(() => driver.peekTitle(), { timeout: 60_000 }).toBe(issue.name);
      await test.step("copy link writes the clipboard and toasts", async () => {
        await driver.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
        await driver.copyPeekLink();
        await expect.poll(() => driver.lastToast(), { timeout: 30_000 }).toMatch(/link copied/i);
        expect(await driver.readClipboard()).toContain(`/browse/${issue.seq}`);
      });
      await test.step("full-screen link resolves to the detail page", async () => {
        const href = await driver.peekFullScreenHref();
        expect(href, "full-screen href").not.toBeNull();
        expect(href!).toContain(`/browse/${issue.seq}`);
      });
      await test.step("quick actions menu lists destructive items", async () => {
        const names = await driver.peekQuickActionNames();
        expect(names.length).toBeGreaterThan(0);
        expect(names.some((entry) => /delete/i.test(entry))).toBe(true);
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-170"], "peek shows a loading state before the issue hydrates"),
  { tag: specTags(["ISS-170"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle peek load ${Date.now()}`);
    // Gate only this issue's fetches so the list page boots normally while
    // the peek waits on its data.
    let release: () => void = () => {};
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    try {
      await test.step("hold the issue fetch and read the loading panel", async () => {
        // A leftover sibling filter would blank the list behind the peek.
        await clearProjectListFilters(seed.workspaceSlug, seed.projectId, session);
        await driver.page.route(
          `**/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/issues/${issue.id}/**`,
          async (route) => {
            await gate;
            await route.continue().catch(() => {});
          }
        );
        // The route stays registered across reloads, so a blank-booted list
        // page can be reloaded until the held peek skeleton shows.
        await driver.page.goto(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues?peekIssueId=${issue.id}`);
        let opened = false;
        for (let round = 0; round < 4 && !opened; round++) {
          if (round > 0) await driver.page.reload();
          try {
            await expect.poll(() => driver.peekOpen(), { timeout: 25_000 }).toBe(true);
            opened = true;
          } catch {
            // Blank boot; the next reload retries it.
          }
        }
        expect(opened).toBe(true);
        // Loading: the panel skeleton is up (with its Close control) but
        // neither the title nor the error shows yet.
        expect(await driver.peekTitle()).toBeNull();
        expect(await driver.peekErrorTitle()).toBeNull();
        expect(await driver.peekCloseVisible()).toBe(true);
      });
      await test.step("the peek still hydrates after the release", async () => {
        release();
        await expect.poll(() => driver.peekTitle(), { timeout: 60_000 }).toBe(issue.name);
      });
    } finally {
      release();
      await driver.page.unrouteAll({ behavior: "wait" }).catch(() => {});
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-171"], "peek error state for a missing work item"),
  { tag: specTags(["ISS-171"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle peek gone ${Date.now()}`);
    const goneId = issue.id;
    await dropIssue(seed, session, issue.id);
    await driver.openPeek(seed.workspaceSlug, seed.projectId, goneId);
    await expect.poll(() => driver.peekErrorTitle(), { timeout: 60_000 }).toBe("Work item does not exist");
    expect(await driver.peekTitle()).toBeNull();
    await test.step("close leaves the error behind", async () => {
      await driver.closePeek();
      await expect.poll(() => driver.peekOpen(), { timeout: 30_000 }).toBe(false);
    });
  }
);

test(
  specTitle(["ISS-172"], "peek properties mirror the detail sidebar"),
  { tag: specTags(["ISS-172"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle peek props ${Date.now()}`);
    try {
      await driver.openPeek(seed.workspaceSlug, seed.projectId, issue.id);
      await expect.poll(() => driver.peekTitle(), { timeout: 60_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      expect(await driver.sidebarProperty("State")).toContain("Todo");
      expect(await driver.sidebarProperty("Priority")).toContain("None");
      expect(await driver.sidebarProperty("Created by")).toContain("Parity Oracle");
      await test.step("a sidebar edit inside the peek saves to the server", async () => {
        await driver.pickPriority("High");
        await expect
          .poll(async () => (await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session))["priority"], {
            timeout: 30_000,
          })
          .toBe("high");
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);
