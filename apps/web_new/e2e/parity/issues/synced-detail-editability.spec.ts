// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWPIDASH-140): a git-synced work item keeps its
// workflow surface editable — the state picker stays enabled and the
// header menu still renders — while only the title (and description)
// editors read as locked, matching the server's locked name/description
// fields. The old app and the new app agree here, so this runs as a
// plain parity row, not a bug: row.
// Rows: ISS-222 (permissions per role; the synced clause).

import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverIssue,
  serverPlantRepositoryBinding,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-222"], "synced work item keeps workflow editable with title locked"),
  { tag: specTags(["ISS-222"]) },
  async ({ driver, seed }) => {
    const tag = `N140 synced ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N140"),
      {},
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    await serverPlantRepositoryBinding(seed.workspaceSlug, projectId, issue.id);
    try {
      await test.step("the server marks the issue synced", async () => {
        const detail = await serverIssue(seed.workspaceSlug, projectId, issue.id, session);
        expect(detail.is_synced).toBe(true);
      });

      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
      // The title renders as static text on a synced item (no input), so
      // settle on the issue key, which both apps render once loaded.
      await expect.poll(() => driver.issueDetailIdentifier(), { timeout: 60_000 }).toMatch(/^[A-Z0-9]{2,}-\d+$/);

      await test.step("the state picker stays enabled on the synced item", async () => {
        expect(await driver.propertyPickerDisabled("State")).toBe(false);
        expect(await driver.propertyPickerDisabled("Priority")).toBe(false);
      });

      await test.step("the header menu still renders on the synced item", async () => {
        const items = await driver.layoutsDetailMenuItems();
        expect(items.length).toBeGreaterThan(0);
      });

      await test.step("the title editor stays read-only on the synced item", async () => {
        expect(await driver.issueTitleInputEnabled()).toBe(false);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session).catch(() => {});
      await serverCleanupProject(seed.workspaceSlug, projectId, session).catch(() => {});
    }
  }
);
