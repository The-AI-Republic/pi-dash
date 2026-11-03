// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-122): state dropdown — project-scoped options,
// no clear row, pick persists server-side.
// Rows: ISS-209 (state dropdown).
import { test, expect } from "../fixtures";
import {
  serverCreateIssueFull,
  serverCreateState,
  serverCleanupIssueWithSession,
  serverDeleteState,
  serverIssue,
  serverPatchIssue,
  serverProjectStates,
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
  specTitle(["ISS-209"], "state dropdown lists project states without a clear row"),
  { tag: specTags(["ISS-209"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 states ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    // A scenario-owned unstarted state joins the seeded states so the pick
    // has somewhere to go; unique name, removed at teardown.
    const extraId = await serverCreateState(seed.workspaceSlug, seed.projectId, `${tag} review`, "unstarted", session);
    try {
      await openOwnIssue(driver, seed, issue.id);

      await test.step("options mirror the project's states with no clear row", async () => {
        const states = await serverProjectStates(seed.workspaceSlug, seed.projectId, session);
        await driver.propertyOpenPicker("State");
        const options = await driver.pickerOptionTexts();
        for (const state of states) expect(options.some((o) => o.includes(state.name))).toBe(true);
        expect(options.some((o) => /no state|clear|none/i.test(o) && !o.includes(`${tag} review`))).toBe(false);
        await driver.pickerClickOutside();
      });

      await test.step("picking a state persists server-side and renders in the row", async () => {
        await driver.propertyOpenPicker("State");
        await driver.pickerPick(`${tag} review`);
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, seed.projectId, issue.id, session)).state_id, {
            timeout: 15_000,
          })
          .toBe(extraId);
        await expect.poll(() => driver.propertyValueText("State"), { timeout: 15_000 }).toContain(`${tag} review`);
      });

      const todo = (await serverProjectStates(seed.workspaceSlug, seed.projectId, session)).find(
        (s) => s.name === "Todo"
      );
      if (todo === undefined) throw new Error("[parity] seeded Todo state is gone.");
      await serverPatchIssue(seed.workspaceSlug, seed.projectId, issue.id, { state_id: todo.id }, session);
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
      await serverDeleteState(seed.workspaceSlug, seed.projectId, extraId, session).catch(() => {
        // A sibling run's issue may sit in this state; reseed clears it.
      });
    }
  }
);
