// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-122): common dropdown behaviors exercised on
// the Priority picker (static enum, no lazy fetch, deterministic options).
// Rows: ISS-207 (variants, keyboard, disabled, empty/loading).
import { test, expect } from "../fixtures";
import {
  serverArchiveIssue,
  serverCreateIssueFull,
  serverCreateState,
  serverCleanupIssueWithSession,
  serverDeleteState,
  serverIssue,
  serverPatchIssue,
  serverProjectStates,
  serverUnarchiveIssue,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

async function openOwnIssue(
  driver: {
    openEntry(): Promise<void>;
    signInWithPassword(e: string, p: string): Promise<void>;
    openIssueDetail(w: string, p: string, i: string): Promise<void>;
  },
  seed: { email: string; password: string; workspaceSlug: string; projectId: string },
  issueId: string
): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPassword(seed.email, seed.password);
  await driver.openIssueDetail(seed.workspaceSlug, seed.projectId, issueId);
}

test(
  specTitle(["ISS-207"], "common dropdown behaviors on the priority picker"),
  { tag: specTags(["ISS-207"]) },
  async ({ driver, seed, page }) => {
    const tag = `NF122 dropdown ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      await openOwnIssue(driver, seed, issue.id);

      await test.step("opens with all five levels and an autofocused search", async () => {
        await driver.propertyOpenPicker("Priority");
        expect(await driver.pickerOptionTexts()).toEqual(["Urgent", "High", "Medium", "Low", "None"]);
        expect(await driver.pickerHasSearch()).toBe(true);
        expect(await driver.pickerSearchFocused()).toBe(true);
        expect(await driver.pickerSearchValue()).toBe("");
      });

      await test.step("search filters, junk shows the empty message, Escape clears the query", async () => {
        await driver.pickerSearch("urg");
        expect(await driver.pickerOptionTexts()).toEqual(["Urgent"]);
        await driver.pickerSearch("zzz");
        expect(await driver.pickerOptionTexts()).toEqual([]);
        expect(await driver.pickerEmptyText()).toContain("No matching results");
        await driver.pickerPressEscape();
        expect(await driver.pickerSearchValue()).toBe("");
        expect(await driver.pickerOpen()).toBe(true);
        expect(await driver.pickerOptionTexts()).toEqual(["Urgent", "High", "Medium", "Low", "None"]);
        await driver.pickerPressEscape();
        expect(await driver.pickerOpen()).toBe(false);
      });

      await test.step("outside click closes the popup", async () => {
        await driver.propertyOpenPicker("Priority");
        expect(await driver.pickerOpen()).toBe(true);
        await driver.pickerClickOutside();
        expect(await driver.pickerOpen()).toBe(false);
      });

      await test.step("keyboard opens, filters and selects; the server stores the pick", async () => {
        await driver.propertyOpenPickerByKeyboard("Priority");
        await driver.pickerSearch("hig");
        expect(await driver.pickerOptionTexts()).toEqual(["High"]);
        await page.keyboard.press("Enter");
        await expect.poll(() => driver.pickerOpen(), { timeout: 15_000 }).toBe(false);
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, seed.projectId, issue.id, session)).priority, {
            timeout: 15_000,
          })
          .toBe("high");
        expect(await driver.propertyValueText("Priority")).toContain("High");
      });

      await test.step("archiving disables the picker and keeps the value visible", async () => {
        // The server only archives completed/cancelled issues, so the issue
        // moves through a scenario-owned completed state first.
        const doneId = await serverCreateState(seed.workspaceSlug, seed.projectId, `${tag} done`, "completed", session);
        try {
          await serverPatchIssue(seed.workspaceSlug, seed.projectId, issue.id, { state_id: doneId }, session);
          await serverArchiveIssue(seed.workspaceSlug, seed.projectId, issue.id, session);
          await driver.openIssueDetail(seed.workspaceSlug, seed.projectId, issue.id);
          expect(await driver.propertyPickerDisabled("Priority")).toBe(true);
          expect(await driver.propertyValueText("Priority")).toContain("High");
          expect(await driver.pickerOpen()).toBe(false);
          await serverUnarchiveIssue(seed.workspaceSlug, seed.projectId, issue.id, session);
          const todo = (await serverProjectStates(seed.workspaceSlug, seed.projectId, session)).find(
            (s) => s.name === "Todo"
          );
          if (todo === undefined) throw new Error("[parity] seeded Todo state is gone.");
          await serverPatchIssue(seed.workspaceSlug, seed.projectId, issue.id, { state_id: todo.id }, session);
        } finally {
          await serverDeleteState(seed.workspaceSlug, seed.projectId, doneId, session).catch(() => {
            // A sibling run's issue may sit in this state; reseed clears it.
          });
        }
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);
