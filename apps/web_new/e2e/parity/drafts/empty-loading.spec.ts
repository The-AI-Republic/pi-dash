// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-32): the empty and no-project states with
// their role-gated actions, plus loading and paging behavior. Rows:
// DRAFT-022, DRAFT-023. Green on apps/web first.
import { test, expect } from "../fixtures";
import {
  ROLE,
  browserSessionCookies,
  projectsDetails,
  serverArchiveProject,
  serverCreateDraft,
  serverDraftsPage,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { draftsHarness, draftsOpenAs, draftsSeat, draftsSettledShape } from "./support";

test(
  specTitle(["DRAFT-022"], "empty and no-project states guide with role-gated actions"),
  { tag: specTags(["DRAFT-022"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d22");

    await test.step("an empty screen with projects offers creation and no chip", async () => {
      await draftsOpenAs(driver, harness);
      expect(await draftsSettledShape(driver)).toBe("empty");
      expect(await driver.draftsMainText()).toContain("Half-written work items");
      expect(await driver.draftsEmptyCreateState()).toBe("enabled");
      expect(await driver.draftsCountChip()).toBeNull();
    });

    await test.step("a project-less guest gets the no-project fallback with a gated action", async () => {
      // The fallback keys off the viewer's own visible project list, not
      // the workspace total: members see every workspace project, guests
      // see only joined ones, so an unseated guest lands on the guidance
      // with the action disabled by the role gate.
      const guest = await draftsSeat(harness, ROLE.GUEST, "parity-d22-guest");
      await draftsOpenAs(driver, harness, guest);
      expect(await draftsSettledShape(driver)).toBe("no-project");
      expect(await driver.draftsMainText()).toContain("No project");
      expect(await driver.draftsHeaderCreateState()).toBe("absent");
      expect(await driver.draftsNoProjectCreateState()).toBe("disabled");
    });

    await test.step("a member with no visible projects gets the fallback with a working action", async () => {
      // Archived projects drop out of the viewer's list, and the default
      // project cannot be deleted, so archiving everything is the only way
      // to leave a member project-less; the action is then enabled.
      const outsider = await draftsSeat(harness, ROLE.MEMBER, "parity-d22-outsider");
      const projects = await projectsDetails(harness.owner, harness.workspaceSlug);
      expect(projects.length).toBeGreaterThan(0);
      for (const project of projects) {
        await serverArchiveProject(harness.workspaceSlug, project.id, harness.owner.cookie);
      }
      await draftsOpenAs(driver, harness, outsider);
      expect(await draftsSettledShape(driver)).toBe("no-project");
      expect(await driver.draftsMainText()).toContain("No project");
      expect(await driver.draftsNoProjectCreateState()).toBe("enabled");
    });
  }
);

test(
  specTitle(["DRAFT-023"], "long lists page through Load More with a delayed skeleton"),
  { tag: specTags(["DRAFT-023"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d23");
    const { owner, workspaceSlug, projectId } = harness;
    for (let i = 1; i <= 51; i++) {
      await serverCreateDraft(workspaceSlug, owner.cookie, {
        name: `D23 Row ${String(i).padStart(3, "0")} ${harness.tag}`,
        project_id: projectId,
      });
    }

    await test.step("the first page holds fifty rows behind Load More", async () => {
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftBlockCount(), { timeout: 120_000 }).toBe(50);
      expect(await driver.draftsLoadMoreVisible()).toBe(true);
      expect(await driver.draftsCountChip()).toBe("51");
      const page = await serverDraftsPage(workspaceSlug, owner.cookie, "");
      expect(page.total).toBe(51);
      expect(page.nextCursor).not.toBeNull();
    });

    await test.step("Load More appends the rest", async () => {
      await driver.draftsLoadMore();
      await expect.poll(() => driver.draftBlockCount(), { timeout: 60_000 }).toBe(51);
    });

    await test.step("a held list shows the loading skeleton first", async () => {
      await driver.draftsDelayList(4000);
      await driver.openAuthenticated(`/${workspaceSlug}/drafts`, browserSessionCookies(owner));
      await expect.poll(() => driver.draftsSkeletonVisible(), { timeout: 30_000 }).toBe(true);
      await expect.poll(() => driver.draftBlockCount(), { timeout: 60_000 }).toBe(50);
    });
  }
);
