// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-122): no real-time updates in the issue
// layer — an externally created or renamed issue does not appear in an
// open list until a reload, while the actor's own UI comment lands
// without one. Row: ISS-221 (no real-time updates).
//
// Observed behavior notes (the inventory row carries them at update
// time): there is no socket/SSE/poll behind lists or detail; the only
// refresh-without-reload is self-initiated (own mutation) plus
// in-memory optimistic updates. The external change here is made over
// the API as the same user, which is mechanically identical to another
// user: a server-side change with no client mutation path.
import { test, expect } from "../fixtures";
import {
  parityApiBase,
  parityProjectIdentifier,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverIssue,
  serverRequestStatus,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-221"], "external changes stay invisible until reload; own comment lands without one"),
  { tag: specTags(["ISS-221"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 rt${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const apiBase = parityApiBase();
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const listed = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} listed`, session);
    const externalName = `${tag} external`;
    const renamedName = `${tag} renamed`;
    let externalId = "";
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.openProjectIssues(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.visibleIssueNames(), { timeout: 60_000 }).toContain(`${tag} listed`);

      await test.step("an externally created issue does not appear without a reload", async () => {
        const created = await serverCreateIssueFull(seed.workspaceSlug, projectId, externalName, session);
        externalId = created.id;
        // The server holds the issue while the open list never learns it.
        expect((await serverIssue(seed.workspaceSlug, projectId, externalId, session)).name).toBe(externalName);
        await new Promise((resolve) => setTimeout(resolve, 2000));
        expect(await driver.visibleIssueNames()).not.toContain(externalName);
      });

      await test.step("an externally renamed issue keeps its stale name without a reload", async () => {
        const res = await serverRequestStatus(
          "PATCH",
          `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/issues/${listed.id}/`,
          session,
          { name: renamedName }
        );
        expect(res.status).toBeGreaterThanOrEqual(200);
        expect(res.status).toBeLessThan(300);
        expect((await serverIssue(seed.workspaceSlug, projectId, listed.id, session)).name).toBe(renamedName);
        await new Promise((resolve) => setTimeout(resolve, 2000));
        const names = await driver.visibleIssueNames();
        expect(names).toContain(`${tag} listed`);
        expect(names).not.toContain(renamedName);
      });

      await test.step("a reload reveals both external changes", async () => {
        await driver.reloadPage();
        await expect.poll(() => driver.visibleIssueNames(), { timeout: 60_000 }).toContain(externalName);
        await expect.poll(() => driver.visibleIssueNames(), { timeout: 60_000 }).toContain(renamedName);
      });

      await test.step("the actor's own UI comment lands without a reload", async () => {
        const body = `${tag} own comment`;
        await driver.openIssueDetail(seed.workspaceSlug, projectId, listed.id);
        await driver.activityPostComment(body);
        await expect
          .poll(async () => (await driver.activityCommentTexts()).some((text) => text.includes(body)), {
            timeout: 30_000,
          })
          .toBe(true);
      });
    } finally {
      await driver.clearIssuePatchFailure().catch(() => {});
      if (externalId !== "") await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, externalId, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, listed.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
