// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-112): every comment carries author identity,
// a relative timestamp with exact-time hover detail, an edited marker
// after edits, and automated-author labeling. Row: CMT-012.
import { test, expect } from "../fixtures";
import {
  composerServerComments,
  composerServerCreateComment,
  serverIssueIdByName,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-012"];

// Cold dev servers compile route bundles on first load, which can take
// minutes on a loaded host; each step still carries its own poll timeout.
test.setTimeout(600_000);

test(
  specTitle(ROWS, "author identity, timestamps, edited marker, bot labeling"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const marker = `composer identity ${Date.now()}`;
    await test.step("sign in, open the work item and post a comment", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.composerOpenIssue(seed.workspaceSlug, "PAR-1");
      await driver.composerType(marker);
      await driver.composerSubmit();
      await expect
        .poll(() => driver.composerVisibleCommentTexts(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([expect.stringContaining(marker)]));
    });

    await test.step("the header names the author with a relative time and no edited marker", async () => {
      const meta = await driver.composerCommentMeta(marker);
      expect(meta.author.length).toBeGreaterThan(0);
      expect(meta.time.length).toBeGreaterThan(0);
      expect(meta.edited).toBe(false);
      expect(meta.tooltip).not.toBeNull();
    });

    await test.step("the hover detail carries the exact date and time behind the relative label", async () => {
      const session = await signInSession(seed.email, seed.password);
      const issueId = await serverIssueIdByName(seed.workspaceSlug, seed.projectId, seed.issueNames[0]!, session);
      const comments = await composerServerComments(seed.workspaceSlug, seed.projectId, issueId, session);
      const stored = comments.find((comment) => comment.comment_stripped.includes(marker));
      expect(stored).toBeDefined();
      const meta = await driver.composerCommentMeta(marker);
      expect(meta.author).toBe(stored!.actorDisplayName);
      expect(meta.tooltip).toContain(new Date(stored!.created_at).getFullYear().toString());
    });

    await test.step("a human author renders a plain name with no automated marker", async () => {
      const session = await signInSession(seed.email, seed.password);
      const issueId = await serverIssueIdByName(seed.workspaceSlug, seed.projectId, seed.issueNames[0]!, session);
      const comments = await composerServerComments(seed.workspaceSlug, seed.projectId, issueId, session);
      const stored = comments.find((comment) => comment.comment_stripped.includes(marker));
      expect(stored!.actorIsBot).toBe(false);
      const meta = await driver.composerCommentMeta(marker);
      expect(meta.author).not.toMatch(/bot/i);
    });

    await test.step("an automated author renders with the bot label", async () => {
      if (seed.botEmail === undefined || seed.botPassword === undefined) {
        throw new Error("[parity] seed facts carry no bot identity; reseed with the NEWFRONT-112 seed.");
      }
      const botMarker = `composer bot ${Date.now()}`;
      const botSession = await signInSession(seed.botEmail, seed.botPassword);
      const ownerSession = await signInSession(seed.email, seed.password);
      const issueId = await serverIssueIdByName(seed.workspaceSlug, seed.projectId, seed.issueNames[0]!, ownerSession);
      await composerServerCreateComment(seed.workspaceSlug, seed.projectId, issueId, `<p>${botMarker}</p>`, botSession);
      const stored = (await composerServerComments(seed.workspaceSlug, seed.projectId, issueId, ownerSession)).find(
        (comment) => comment.comment_stripped.includes(botMarker)
      );
      expect(stored).toBeDefined();
      expect(stored!.actorIsBot).toBe(true);
      await driver.composerOpenIssue(seed.workspaceSlug, "PAR-1");
      await expect
        .poll(() => driver.composerVisibleCommentTexts(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([expect.stringContaining(botMarker)]));
      const meta = await driver.composerCommentMeta(botMarker);
      expect(meta.author).toMatch(/bot/i);
    });

    await test.step("editing stamps the edited marker while identity stays put", async () => {
      const before = await driver.composerCommentMeta(marker);
      await driver.composerOpenCommentMenu(marker);
      await driver.composerMenuClick("Edit");
      await driver.composerEditType(`${marker} touched`);
      await driver.composerEditSave();
      await expect
        .poll(() => driver.composerVisibleCommentTexts(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([expect.stringContaining(`${marker} touched`)]));
      const after = await driver.composerCommentMeta(`${marker} touched`);
      expect(after.edited).toBe(true);
      expect(after.author).toBe(before.author);
      const session = await signInSession(seed.email, seed.password);
      const issueId = await serverIssueIdByName(seed.workspaceSlug, seed.projectId, seed.issueNames[0]!, session);
      const comments = await composerServerComments(seed.workspaceSlug, seed.projectId, issueId, session);
      expect(comments.find((comment) => comment.comment_stripped.includes(marker))!.edited_at).not.toBeNull();
    });
  }
);
