// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-114): the merged activity feed. The old app
// never reads the merged history endpoint (it 500s — bug NEWFRONT-128, the
// merged branch subscripts uns serialized model rows); it reads the
// property and comment splits and merges them client-side, oldest-first by
// default. This scenario drives one comment plus one title change through
// the UI and proves the on-screen feed merges both sources in chronological
// order with actor, timestamp and per-field content, and that entry links
// navigate. Row: CMT-013.
import { test, expect } from "../fixtures";
import { serverCreateIssue, serverCleanupIssue, serverHistory, signInSessionWithRetry } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-013"];
const TIME_HINT = /(ago|just now|\b\d{4}\b|\b(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\b|\d{1,2}:\d{2})/i;

test(
  specTitle(ROWS, "activity feed merges property changes and comments in order (bug: NEWFRONT-128)"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    // The scratch stack and runner host are shared with concurrent
    // parity runs, so wall-clock time varies wildly; the assertions
    // below are all poll-based and correct at any speed.
    test.slow();
    const stamp = Date.now();
    const issueName = `Activity feed probe ${stamp}`;
    const commentBody = `activity probe comment ${stamp}`;
    const renamedTitle = `${issueName} renamed`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const issueId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, issueName);
    try {
      await test.step("sign in and open the fresh work item", async () => {
        await driver.activitySignIn(seed.email, seed.password, seed.workspaceSlug);
        await driver.activityOpenIssueDetail(seed.workspaceSlug, seed.projectId, issueId);
      });

      await test.step("the creation entry renders with actor and timestamp", async () => {
        await expect.poll(() => driver.activityEntryTexts(), { timeout: 60_000 }).toHaveLength(1);
        const texts = await driver.activityEntryTexts();
        expect(texts[0]).toContain("Parity Oracle");
        expect(texts[0]).toMatch(TIME_HINT);
      });

      await test.step("post a comment through the composer", async () => {
        await driver.activityComposerType(commentBody);
        await driver.activityComposerSubmit();
        await expect
          .poll(() => driver.activityEntryTexts(), { timeout: 60_000 })
          .toEqual(expect.arrayContaining([expect.stringContaining(commentBody)]));
      });

      await test.step("rename the title through the header", async () => {
        await driver.activityRenameTitle(renamedTitle);
        await expect
          .poll(() => driver.activityEntryTexts(), { timeout: 60_000 })
          .toEqual(expect.arrayContaining([expect.stringContaining(renamedTitle)]));
      });

      await test.step("the merged feed stays chronological with actor and timestamp per entry", async () => {
        const texts = await driver.activityEntryTexts();
        expect(texts).toHaveLength(3);
        // Oldest first: creation, then the comment, then the rename.
        expect(texts[1]).toContain(commentBody);
        expect(texts[2]).toContain(renamedTitle);
        for (const entry of texts) {
          expect(entry).toContain("Parity Oracle");
          expect(entry).toMatch(TIME_HINT);
        }
      });

      await test.step("entry links navigate to the referenced item", async () => {
        const url = await driver.activityOpenFirstEntryLink();
        expect(url).toMatch(/profile/);
      });

      await test.step("the server agrees: splits 200, merged 500 (NEWFRONT-128)", async () => {
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
        expect(props.status).toBe(200);
        expect(comments.status).toBe(200);
        expect(props.entries).toHaveLength(2);
        expect(comments.entries).toHaveLength(1);
        for (const entry of [...props.entries, ...comments.entries]) {
          expect(typeof entry.id).toBe("string");
          expect(typeof entry.created_at).toBe("string");
        }
        expect(comments.entries[0]?.comment_html ?? comments.entries[0]?.comment).toBeDefined();
        const merged = await serverHistory(seed.workspaceSlug, seed.projectId, issueId, session, "");
        expect(merged.status).toBe(500);
      });
    } finally {
      await serverCleanupIssue(seed.workspaceSlug, seed.projectId, issueId, seed.email, seed.password, session);
    }
  }
);
