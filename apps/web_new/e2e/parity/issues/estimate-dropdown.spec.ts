// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-122): estimate dropdown — points pick and
// clear, search narrowing, TIME-system hour/minute rendering, and the
// row's absence when the project has no estimate system.
// Rows: ISS-216 (estimate dropdown).
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCreateEstimate,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverIssue,
  serverSetProjectEstimate,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-216"], "estimate dropdown assigns, searches and clears points"),
  { tag: specTags(["ISS-216"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 estimates ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    // The Estimate row only renders once the project points at an estimate
    // system, so the scenario owns a project and activates its own system.
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, "Points", ["1", "2", "3"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    try {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);

      await test.step("unestimated issues offer a None placeholder", async () => {
        expect(await driver.propertyRowPresent("Estimate")).toBe(true);
        await expect.poll(() => driver.propertyValueText("Estimate"), { timeout: 15_000 }).toContain("None");
      });

      await test.step("the picker lists No estimate plus the points", async () => {
        await driver.propertyOpenPicker("Estimate");
        expect(await driver.pickerHasSearch()).toBe(true);
        const options = await driver.pickerOptionTexts();
        expect(options.some((o) => /no estimate/i.test(o))).toBe(true);
        expect(options.some((o) => o.trim() === "1")).toBe(true);
        expect(options.some((o) => o.trim() === "3")).toBe(true);
        await driver.pickerClickOutside();
      });

      await test.step("search narrows options and a miss shows the empty message", async () => {
        await driver.propertyOpenPicker("Estimate");
        await driver.pickerSearch("2");
        const narrowed = await driver.pickerOptionTexts();
        expect(narrowed.some((o) => o.trim() === "2")).toBe(true);
        expect(narrowed.some((o) => o.trim() === "3")).toBe(false);
        await driver.pickerSearch("2 no-such-point");
        expect(await driver.pickerOptionTexts()).toEqual([]);
        expect(await driver.pickerEmptyText()).toContain("No matching results");
        await driver.pickerPressEscape();
        await driver.pickerClickOutside();
      });

      await test.step("picking a point persists and renders in the row", async () => {
        const wanted = system.points.find((p) => p.value === "3");
        expect(wanted).toBeDefined();
        await driver.propertyOpenPicker("Estimate");
        await driver.pickerPick("3");
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).estimate_point, {
            timeout: 15_000,
          })
          .toBe(wanted?.id);
        await expect.poll(() => driver.propertyValueText("Estimate"), { timeout: 15_000 }).toContain("3");
      });

      await test.step("No estimate clears the point", async () => {
        await driver.propertyOpenPicker("Estimate");
        await driver.pickerPick("No estimate");
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).estimate_point, {
            timeout: 15_000,
          })
          .toBe(null);
        await expect.poll(() => driver.propertyValueText("Estimate"), { timeout: 15_000 }).toContain("None");
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-216"], "estimate dropdown renders TIME systems as hours and minutes"),
  { tag: specTags(["ISS-216"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 time est ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, "Time", ["30", "90"], session, "time");
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    try {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);

      await test.step("minute values render as hours and minutes", async () => {
        await driver.propertyOpenPicker("Estimate");
        const options = await driver.pickerOptionTexts();
        expect(options.some((o) => o.includes("30m"))).toBe(true);
        expect(options.some((o) => o.includes("1h") && o.includes("30m"))).toBe(true);
        const wanted = system.points.find((p) => p.value === "90");
        await driver.pickerPick("1h");
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).estimate_point, {
            timeout: 15_000,
          })
          .toBe(wanted?.id);
        await expect.poll(() => driver.propertyValueText("Estimate"), { timeout: 15_000 }).toContain("1h");
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-216"], "projects without an estimate system show no Estimate row"),
  { tag: specTags(["ISS-216"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 no est ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    // No estimate system is created or activated on this project, so the
    // sidebar omits the row entirely (there is no picker to open).
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    try {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
      expect(await driver.propertyRowPresent("Estimate")).toBe(false);
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
