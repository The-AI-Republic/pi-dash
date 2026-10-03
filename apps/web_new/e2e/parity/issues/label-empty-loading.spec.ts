// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-122): labels empty and loading states —
// a fresh project shows the "No labels yet" empty state whose action
// opens the create form, and the list shows a skeleton while the
// label endpoints are in flight. Row: ISS-230 (labels empty & loading
// states).
//
// Observed behavior notes (inventory row carries them at update time):
// the empty state renders once the store has any label answer (project
// or workspace fetch); the skeleton is a four-row pulse shown while
// neither fetch has answered.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupProject,
  serverCreateProjectWithFlags,
  serverLabels,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-230"], "labels page shows the empty state and its action opens the create form"),
  { tag: specTags(["ISS-230"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 lble ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);

      await test.step("a fresh project shows the empty state", async () => {
        expect(await serverLabels(seed.workspaceSlug, projectId, session)).toEqual([]);
        await expect.poll(() => driver.settingsLabelsEmptyTitle(), { timeout: 15_000 }).toBe("No labels yet");
        expect(await driver.settingsLabelsAddVisible()).toBe(true);
        expect(await driver.settingsLabelsNames()).toEqual([]);
      });

      await test.step("the empty-state action opens the create form", async () => {
        await driver.settingsLabelsEmptyAction();
        expect(await driver.settingsLabelsFormVisible()).toBe(true);
        await driver.settingsLabelsCancelForm();
        await expect.poll(() => driver.settingsLabelsEmptyTitle(), { timeout: 15_000 }).toBe("No labels yet");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-230"], "labels page shows a skeleton while the label lists load"),
  { tag: specTags(["ISS-230"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 lbll ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      // Hold the list answers so the skeleton stays up observably long;
      // the empty state below proves the held answers then landed.
      await driver.settingsLabelsDelayLoad(3_000);
      await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.settingsLabelsSkeletonVisible(), { timeout: 20_000 }).toBe(true);
      await expect.poll(() => driver.settingsLabelsEmptyTitle(), { timeout: 20_000 }).toBe("No labels yet");
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
