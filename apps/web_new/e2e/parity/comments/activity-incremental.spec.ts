// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-114): incremental feed fetch. After a property
// change through the UI, the app refetches the property history with
// `created_at__gt` set to the newest entry it already knows and appends
// only the delta — the new entry appears in place with no page reload.
// The scenario records the refetch URLs, proves the delta parameter, and
// proves the UI and server state agree. Row: CMT-017.
import { test, expect } from "../fixtures";
import { serverCreateIssue, serverCleanupIssue, serverHistory, signInSessionWithRetry } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-017"];

test(
  specTitle(ROWS, "activity incremental fetch picks up new entries without reload"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    // The scratch stack and runner host are shared with concurrent
    // parity runs, so wall-clock time varies wildly; the assertions
    // below are all poll-based and correct at any speed.
    test.slow();
    const stamp = Date.now();
    const issueName = `Incremental probe ${stamp}`;
    const renamedTitle = `${issueName} renamed`;
    const commentBody = `incremental probe comment ${stamp}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const issueId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, issueName);
    try {
      const before = await serverHistory(
        seed.workspaceSlug,
        seed.projectId,
        issueId,
        session,
        "?activity_type=issue-property"
      );
      expect(before.status).toBe(200);
      expect(before.entries).toHaveLength(1);
      const newestKnown = before.entries[0]?.created_at;
      expect(typeof newestKnown).toBe("string");

      await test.step("sign in and open the fresh work item", async () => {
        await driver.activitySignIn(seed.email, seed.password, seed.workspaceSlug);
        await driver.activityOpenIssueDetail(seed.workspaceSlug, seed.projectId, issueId);
        await expect.poll(() => driver.activityEntryTexts(), { timeout: 60_000 }).toHaveLength(1);
      });
      const urlBefore = driver.page.url();

      await test.step("rename the title and watch the delta arrive in place", async () => {
        const refetchUrls: string[] = [];
        await driver.page.route("**/history/**", async (route) => {
          refetchUrls.push(route.request().url());
          await route.continue();
        });
        await driver.activityRenameTitle(renamedTitle);
        // The new entry renders without any reload or navigation.
        await expect
          .poll(() => driver.activityEntryTexts(), { timeout: 60_000 })
          .toEqual(expect.arrayContaining([expect.stringContaining(renamedTitle)]));
        expect(driver.page.url()).toBe(urlBefore);
        // The property refetch carried the incremental cursor.
        const deltas = refetchUrls.filter(
          (url) => url.includes("activity_type=issue-property") && url.includes("created_at__gt=")
        );
        expect(deltas.length).toBeGreaterThan(0);
        const cursor = decodeURIComponent(deltas[0]?.split("created_at__gt=")[1]?.split("&")[0] ?? "");
        expect(cursor).toBe(newestKnown);
      });

      await test.step("a composer comment joins the live feed too", async () => {
        await driver.activityComposerType(commentBody);
        await driver.activityComposerSubmit();
        await expect
          .poll(() => driver.activityEntryTexts(), { timeout: 60_000 })
          .toEqual(expect.arrayContaining([expect.stringContaining(commentBody)]));
        expect(driver.page.url()).toBe(urlBefore);
      });

      await test.step("the server agrees with the screen", async () => {
        const props = await serverHistory(
          seed.workspaceSlug,
          seed.projectId,
          issueId,
          session,
          "?activity_type=issue-property"
        );
        const comments = await serverHistory(
          seed.workspaceSlug,
          seed.projectId,
          issueId,
          session,
          "?activity_type=issue-comment"
        );
        expect(props.entries).toHaveLength(2);
        expect(comments.entries).toHaveLength(1);
        expect(Date.parse(props.entries[1]?.created_at ?? "")).toBeGreaterThan(Date.parse(String(newestKnown)));
        const texts = await driver.activityEntryTexts();
        expect(texts).toHaveLength(3);
      });
    } finally {
      await serverCleanupIssue(seed.workspaceSlug, seed.projectId, issueId, seed.email, seed.password, session);
    }
  }
);
