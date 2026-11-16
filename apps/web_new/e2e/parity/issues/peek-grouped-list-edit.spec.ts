// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario: edit properties from the peek while the project list is
// grouped by state. The everyday path: the list (grouped by state, the
// default) is on screen, the user opens an item in the peek and changes its
// state or priority. The new app paints the change into every cached copy
// of the list before the PATCH goes out; a cached copy in a shape the
// painter did not expect threw before the request was sent, so the edit
// failed with "Couldn't update the work item" and nothing reached the
// server (NEWPIDASH-251 in prod, fixed by #3351). The detail-page specs
// never load a list first, so they could not see it.
// Rows: ISS-076 (group by), ISS-167 (peek), ISS-209 (state dropdown).
import { test, expect } from "../fixtures";
import {
  getProjectUserProperties,
  serverCleanupIssueWithSession,
  serverCreateIssueFull,
  serverCreateState,
  serverDeleteState,
  serverIssue,
  serverPatchProjectUserProperties,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["ISS-076", "ISS-167", "ISS-209"];

test(
  specTitle(ROWS, "peek edits persist and re-group the row while the list is grouped by state"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const tag = `Peek grouped ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    // A scenario-owned state is the move target, so the source and target
    // groups are always distinct and the target group starts empty.
    const targetName = `${tag} doing`;
    const targetId = await serverCreateState(seed.workspaceSlug, seed.projectId, targetName, "started", session);
    const before = await getProjectUserProperties(seed.workspaceSlug, seed.projectId, session);
    try {
      await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        display_filters: { layout: "list", group_by: "state", order_by: "sort_order", show_empty_groups: true },
      });

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      // The peek deep link mounts the grouped list underneath the panel, so
      // the grouped list query is cached before any edit.
      await driver.openPeek(seed.workspaceSlug, seed.projectId, issue.id);

      const sourceGroup = await test.step("the list is grouped by state and holds the item", async () => {
        await expect.poll(() => driver.layoutsListGroups(), { timeout: 30_000 }).toContain(targetName);
        let found: string | null = null;
        await expect
          .poll(
            async () => {
              for (const group of await driver.layoutsListGroups()) {
                if ((await driver.layoutsListGroupIssueNames(group)).includes(issue.name)) {
                  found = group;
                  return true;
                }
              }
              return false;
            },
            { timeout: 30_000 }
          )
          .toBe(true);
        if (found === null) throw new Error("[parity] the created item is in no list group.");
        return found as string;
      });
      expect(sourceGroup).not.toBe(targetName);

      await test.step("changing the state in the peek persists server-side", async () => {
        await driver.propertyOpenPicker("State");
        await driver.pickerPick(targetName);
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, seed.projectId, issue.id, session)).state_id, {
            timeout: 15_000,
          })
          .toBe(targetId);
        await expect.poll(() => driver.propertyValueText("State"), { timeout: 15_000 }).toContain(targetName);
      });

      await test.step("the row moves to the new state's group", async () => {
        await expect
          .poll(() => driver.layoutsListGroupIssueNames(targetName), { timeout: 15_000 })
          .toContain(issue.name);
        expect(await driver.layoutsListGroupIssueNames(sourceGroup)).not.toContain(issue.name);
      });

      await test.step("changing the priority in the peek persists server-side", async () => {
        await driver.propertyOpenPicker("Priority");
        await driver.pickerPick("Urgent");
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, seed.projectId, issue.id, session)).priority, {
            timeout: 15_000,
          })
          .toBe("urgent");
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
      await serverDeleteState(seed.workspaceSlug, seed.projectId, targetId, session).catch(() => {
        // A sibling run's item may sit in this state; reseed clears it.
      });
      const display = before["display_filters"];
      if (display !== null && typeof display === "object") {
        await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
          display_filters: display as Record<string, unknown>,
        });
      }
    }
  }
);
