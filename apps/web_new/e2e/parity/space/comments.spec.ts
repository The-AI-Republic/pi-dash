// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-451): public-board comments — section gating
// (SPC-049), rows (SPC-050), folded comments (SPC-051), speaker labels
// (SPC-052), posting with attachments (SPC-053), mentions (SPC-054),
// owner menu (SPC-055), edits (SPC-056) and comment reactions (SPC-057).
// Each scenario flips the board flags through the publish API and restores
// them, so reruns on one seeded stack stay green. Seven scenarios pin
// current bugs (NEWFRONT-459/460/461/465/466/467); the rows record the
// intent. The 459 age scenario pins the bare line on the old app and
// proves the relative age on web_new.
import { fileURLToPath } from "node:url";
import { test, expect } from "../fixtures";
import {
  serverSpaceComments,
  serverSpaceCommentReact,
  serverSpaceDeleteComment,
  serverSpaceIssueIdByName,
  serverSpaceMembers,
  serverSpacePlantBot,
  serverSpacePostComment,
  serverSpaceBoardFlags,
  serverSpaceSetBoardFlags,
  sessionBrowserCookies,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParitySeedFacts } from "../drivers/parity-driver";

// Cold dev servers compile route bundles on first load, which can take
// minutes on a loaded host; each step still carries its own poll timeout.
test.setTimeout(600_000);

const uploadFile = fileURLToPath(new URL("./space-upload.png", import.meta.url));

function requireAnchor(seed: ParitySeedFacts): string {
  if (!seed.spaceAnchor) throw new Error("[parity] seed facts carry no spaceAnchor; re-run the stack seed step.");
  return seed.spaceAnchor;
}

function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.guestEmail || !seed.guestPassword)
    throw new Error("[parity] seed facts carry no guest identity; re-run the stack seed step.");
  return { email: seed.guestEmail, password: seed.guestPassword };
}

/** Delete a comment only when one exists (a missing row answers 500, not 404). */
async function deleteCommentIfPresent(
  anchor: string,
  issueId: string,
  commentId: string,
  session: string
): Promise<void> {
  try {
    await serverSpaceDeleteComment(anchor, issueId, commentId, session);
  } catch (error) {
    if (!/HTTP (404|500)/.test(error instanceof Error ? error.message : "")) throw error;
  }
}

async function setFlags(
  seed: ParitySeedFacts,
  flags: { comments: boolean; reactions: boolean; votes: boolean },
  session: string
) {
  const before = await serverSpaceBoardFlags(seed.workspaceSlug, seed.projectId, session);
  await serverSpaceSetBoardFlags(seed.workspaceSlug, seed.projectId, session, flags);
  return before;
}

async function restoreFlags(
  seed: ParitySeedFacts,
  before: { comments: boolean; reactions: boolean; votes: boolean },
  session: string
): Promise<void> {
  await serverSpaceSetBoardFlags(seed.workspaceSlug, seed.projectId, session, {
    comments: before.comments,
    reactions: before.reactions,
    votes: before.votes,
  });
}

test(
  specTitle(["SPC-049"], "comments section follows the publisher flag and sign-in state"),
  { tag: specTags(["SPC-049"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[0]!;
    const marker = `section matrix ${Date.now()}`;
    const admin = await signInSession(seed.email, seed.password);
    const before = await setFlags(seed, { comments: false, reactions: false, votes: false }, admin);
    let commentId: string | null = null;
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      // Ghost-proof the exact counts below: a run killed mid-test leaves
      // markers that the flag-gated list hides until the flags flip on.
      await setFlags(seed, { comments: true, reactions: false, votes: false }, admin);
      for (const ghost of await serverSpaceComments(anchor, issueId)) {
        await deleteCommentIfPresent(anchor, issueId, ghost.id, admin);
      }
      await setFlags(seed, { comments: false, reactions: false, votes: false }, admin);
      await driver.page.context().addCookies(sessionBrowserCookies(admin));

      await test.step("disabled boards hide the whole section from signed-in visitors", async () => {
        await driver.spaceCommentOpenIssue(anchor, issueName);
        expect(await driver.spaceCommentSection()).toEqual({
          sectionVisible: false,
          threadCount: 0,
          composerVisible: false,
          signInInviteVisible: false,
        });
      });

      await test.step("enabled boards show the thread plus the composer to signed-in visitors", async () => {
        await setFlags(seed, { comments: true, reactions: false, votes: false }, admin);
        commentId = (await serverSpacePostComment(anchor, issueId, `<p>${marker}</p>`, marker, admin)).id;
        await driver.spaceCommentOpenIssue(anchor, issueName);
        expect(await driver.spaceCommentSection()).toEqual({
          sectionVisible: true,
          threadCount: 1,
          composerVisible: true,
          signInInviteVisible: false,
        });
      });

      await test.step("anonymous visitors read the thread and get a sign-in invitation", async () => {
        await driver.page.context().clearCookies();
        await driver.spaceCommentOpenIssue(anchor, issueName);
        expect(await driver.spaceCommentSection()).toEqual({
          sectionVisible: true,
          threadCount: 1,
          composerVisible: false,
          signInInviteVisible: true,
        });
      });

      await test.step("embedded boards invite no sign-in", async () => {
        await driver.spaceCommentOpenEmbed(anchor, issueName);
        expect(await driver.spaceCommentEmbedSection()).toEqual({
          sectionVisible: true,
          threadCount: 1,
          composerVisible: false,
          signInInviteVisible: false,
        });
      });

      await test.step("the server thread matches the screens", async () => {
        const comments = await serverSpaceComments(anchor, issueId);
        expect(comments.map((comment) => comment.stripped)).toEqual([marker]);
      });
    } finally {
      if (commentId !== null) {
        const issueId = await serverSpaceIssueIdByName(anchor, issueName).catch(() => null);
        if (issueId !== null) await deleteCommentIfPresent(anchor, issueId, commentId, admin);
      }
      await restoreFlags(seed, before, admin);
    }
  }
);

test(
  specTitle(["SPC-049"], "bug: NEWFRONT-465 comment invitation returns to the bare board"),
  { tag: specTags(["SPC-049"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[0]!;
    const admin = await signInSession(seed.email, seed.password);
    const before = await setFlags(seed, { comments: true, reactions: false, votes: false }, admin);
    try {
      await test.step("open the issue signed out from the kanban board", async () => {
        await driver.spaceCommentOpenIssue(anchor, issueName, "kanban");
        expect((await driver.spaceCommentSection()).signInInviteVisible).toBe(true);
      });

      await test.step("the invitation keeps the board but drops the issue and the view", async () => {
        // Intended on web_new (bug NEWFRONT-465 fixed): the invitation
        // carries the whole board position, view and issue included.
        if (driver.target === "web_new") {
          const issueId = await serverSpaceIssueIdByName(anchor, issueName);
          expect(await driver.spaceCommentInviteReturn()).toEqual({ anchor, view: "kanban", issueId });
        } else {
          expect(await driver.spaceCommentInviteReturn()).toEqual({ anchor, view: null, issueId: null });
        }
      });
    } finally {
      await restoreFlags(seed, before, admin);
    }
  }
);

test(
  specTitle(["SPC-049"], "bug: NEWFRONT-461 full-screen peek shows the comments shell while disabled"),
  { tag: specTags(["SPC-049"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[0]!;
    const admin = await signInSession(seed.email, seed.password);
    const before = await setFlags(seed, { comments: false, reactions: false, votes: false }, admin);
    try {
      if (driver.target === "web_new") {
        // Intended behavior (NEWFRONT-461): commenting disabled hides the
        // whole section in every presentation — no heading, no thread, no
        // composer, and no invitation, signed-in or anonymous.
        await test.step("full-screen hides the whole section while disabled", async () => {
          await driver.page.context().addCookies(sessionBrowserCookies(admin));
          await driver.spaceCommentOpenIssue(anchor, issueName);
          await driver.spaceCommentPeekMode("full");
          await expect.poll(() => driver.spacePeekMode(), { timeout: 30_000 }).toBe("full");
          expect(await driver.spaceCommentSection()).toEqual({
            sectionVisible: false,
            threadCount: 0,
            composerVisible: false,
            signInInviteVisible: false,
          });
        });

        await test.step("anonymous full-screen offers no sign-in invitation", async () => {
          await driver.page.context().clearCookies();
          await driver.spaceCommentOpenIssue(anchor, issueName);
          await driver.spaceCommentPeekMode("full");
          await expect.poll(() => driver.spacePeekMode(), { timeout: 30_000 }).toBe("full");
          expect(await driver.spaceCommentSection()).toEqual({
            sectionVisible: false,
            threadCount: 0,
            composerVisible: false,
            signInInviteVisible: false,
          });
        });
        return;
      }
      await driver.page.context().addCookies(sessionBrowserCookies(admin));

      await test.step("full-screen renders the shell with an empty thread while disabled", async () => {
        await driver.spaceCommentOpenIssue(anchor, issueName);
        await driver.spaceCommentPeekMode("full");
        await expect
          .poll(() => driver.spaceCommentSection().then((section) => section.sectionVisible), { timeout: 30_000 })
          .toBe(true);
        expect(await driver.spaceCommentSection()).toEqual({
          sectionVisible: true,
          threadCount: 0,
          composerVisible: false,
          signInInviteVisible: false,
        });
      });

      await test.step("anonymous full-screen even invites a sign-in to nowhere", async () => {
        await driver.page.context().clearCookies();
        await driver.spaceCommentOpenIssue(anchor, issueName);
        await driver.spaceCommentPeekMode("full");
        await expect
          .poll(() => driver.spaceCommentSection().then((section) => section.signInInviteVisible), { timeout: 30_000 })
          .toBe(true);
      });
    } finally {
      await restoreFlags(seed, before, admin);
    }
  }
);

test(
  specTitle(["SPC-050"], "comment rows show identity, bot marking and formatted content"),
  { tag: specTags(["SPC-050"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[1]!;
    const marker = `row identity ${Date.now()}`;
    const botMarker = `row bot ${Date.now()}`;
    const admin = await signInSession(seed.email, seed.password);
    const before = await setFlags(seed, { comments: true, reactions: false, votes: false }, admin);
    const posted: { issueId: string; id: string; session: string }[] = [];
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      const bot = await serverSpacePlantBot();
      const botSession = await signInSession(bot.email, bot.password);
      const adminComment = await serverSpacePostComment(
        anchor,
        issueId,
        `<p>${marker} with <strong>bold</strong></p>`,
        `${marker} with bold`,
        admin
      );
      const botComment = await serverSpacePostComment(anchor, issueId, `<p>${botMarker}</p>`, botMarker, botSession);
      posted.push(
        { issueId, id: adminComment.id, session: admin },
        { issueId, id: botComment.id, session: botSession }
      );

      await test.step("sign in and open the issue", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenIssue(anchor, issueName);
      });

      await test.step("rows carry author, marking, avatar fallback and formatted bodies", async () => {
        await expect
          .poll(() => driver.spaceCommentRows().then((rows) => rows.length), { timeout: 60_000 })
          .toBeGreaterThanOrEqual(2);
        const rows = await driver.spaceCommentRows();
        const human = rows.find((row) => row.contentText.includes(marker))!;
        expect(human).toBeDefined();
        expect(human.author).toBe(adminComment.displayName);
        expect(human.botMark).toBe(false);
        expect(human.avatarKind).toBe("initial");
        expect(human.contentText).toContain("bold");
        const machine = rows.find((row) => row.contentText.includes(botMarker))!;
        expect(machine).toBeDefined();
        expect(machine.author).toBe(`${bot.firstName} Bot`);
        expect(machine.author).not.toBe(bot.displayName);
        expect(machine.botMark).toBe(true);
      });

      await test.step("the server agrees with the rows", async () => {
        const comments = await serverSpaceComments(anchor, issueId);
        const storedBot = comments.find((comment) => comment.id === botComment.id)!;
        expect(storedBot.isBot).toBe(true);
        expect(storedBot.displayName).toBe(bot.displayName);
        expect(comments.find((comment) => comment.id === adminComment.id)!.html).toContain("bold");
      });
    } finally {
      for (const row of posted) await deleteCommentIfPresent(anchor, row.issueId, row.id, row.session);
      await restoreFlags(seed, before, admin);
    }
  }
);

test(
  specTitle(["SPC-050"], "bug: NEWFRONT-459 comment rows carry no relative age"),
  { tag: specTags(["SPC-050"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[1]!;
    const marker = `row age ${Date.now()}`;
    const admin = await signInSession(seed.email, seed.password);
    const before = await setFlags(seed, { comments: true, reactions: false, votes: false }, admin);
    let commentId: string | null = null;
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      commentId = (await serverSpacePostComment(anchor, issueId, `<p>${marker}</p>`, marker, admin)).id;

      await test.step("sign in and open the issue", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenIssue(anchor, issueName);
      });

      await test.step("the age line matches its target with the timestamp stored server-side", async () => {
        await expect
          .poll(() => driver.spaceCommentRows().then((rows) => rows.length), { timeout: 60_000 })
          .toBeGreaterThanOrEqual(1);
        const row = (await driver.spaceCommentRows()).find((candidate) => candidate.contentText.includes(marker))!;
        expect(row).toBeDefined();
        // bug:NEWFRONT-459 — the old row interpolates an empty age, so the
        // line reads bare; web_new implements the intent (the shared timeAgo
        // ladder, matching the workspace rows), so the line carries a
        // relative age. Fresh rows read "just now" but may tick over on a
        // slow run, so the new target accepts any ladder rung.
        if (driver.target === "web_new") {
          expect(row.ageText).toMatch(
            /^commented (just now|\d+ minutes? ago|\d+ hours? ago|yesterday|\d+ days ago|\d+ weeks? ago|\d+ months? ago|\d+ years? ago)$/
          );
        } else {
          expect(row.ageText).toBe("commented");
        }
        const stored = (await serverSpaceComments(anchor, issueId)).find((comment) => comment.id === commentId)!;
        expect(stored.createdAt).not.toBe("");
      });
    } finally {
      if (commentId !== null) {
        const issueId = await serverSpaceIssueIdByName(anchor, issueName).catch(() => null);
        if (issueId !== null) await deleteCommentIfPresent(anchor, issueId, commentId, admin);
      }
      await restoreFlags(seed, before, admin);
    }
  }
);

test(
  specTitle(["SPC-051"], "folded comments stay collapsed until expanded"),
  { tag: specTags(["SPC-051"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[1]!;
    const marker = `folded body ${Date.now()}`;
    const admin = await signInSession(seed.email, seed.password);
    const before = await setFlags(seed, { comments: true, reactions: false, votes: false }, admin);
    let commentId: string | null = null;
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      commentId = (
        await serverSpacePostComment(anchor, issueId, `<p>${marker}</p>`, marker, admin, { labels: ["fold"] })
      ).id;

      await test.step("sign in and open the issue", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenIssue(anchor, issueName);
      });

      await test.step("the folded row hides its body until expanded", async () => {
        await expect
          .poll(() => driver.spaceCommentRows().then((rows) => rows.length), { timeout: 60_000 })
          .toBeGreaterThanOrEqual(1);
        const rows = await driver.spaceCommentRows();
        const index = rows.findIndex((row) => row.folded);
        expect(index).toBeGreaterThanOrEqual(0);
        expect(rows[index]!.contentText).toBe("");
        await driver.spaceCommentToggleFold(index);
        await expect
          .poll(() => driver.spaceCommentRows().then((candidates) => candidates[index]!.contentText), {
            timeout: 30_000,
          })
          .toContain(marker);
        await driver.spaceCommentToggleFold(index);
        await expect
          .poll(() => driver.spaceCommentRows().then((candidates) => candidates[index]!.contentText), {
            timeout: 30_000,
          })
          .toBe("");
      });

      await test.step("the server marks the row folded", async () => {
        const stored = (await serverSpaceComments(anchor, issueId)).find((comment) => comment.id === commentId)!;
        expect(stored.labels).toContain("fold");
      });
    } finally {
      if (commentId !== null) {
        const issueId = await serverSpaceIssueIdByName(anchor, issueName).catch(() => null);
        if (issueId !== null) await deleteCommentIfPresent(anchor, issueId, commentId, admin);
      }
      await restoreFlags(seed, before, admin);
    }
  }
);

test(
  specTitle(["SPC-052"], "machine-authored comments carry their speaker label"),
  { tag: specTags(["SPC-052"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[1]!;
    const marker = `agent body ${Date.now()}`;
    const humanMarker = `human body ${Date.now()}`;
    const admin = await signInSession(seed.email, seed.password);
    const before = await setFlags(seed, { comments: true, reactions: false, votes: false }, admin);
    const posted: { issueId: string; id: string }[] = [];
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      const agent = await serverSpacePostComment(anchor, issueId, `<p>${marker}</p>`, marker, admin, {
        speaker_type: "agent",
        speaker_label: "Probe Agent",
      });
      const human = await serverSpacePostComment(anchor, issueId, `<p>${humanMarker}</p>`, humanMarker, admin);
      posted.push({ issueId, id: agent.id }, { issueId, id: human.id });

      await test.step("sign in and open the issue", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenIssue(anchor, issueName);
      });

      await test.step("only the machine row carries a speaker label", async () => {
        await expect
          .poll(() => driver.spaceCommentRows().then((rows) => rows.length), { timeout: 60_000 })
          .toBeGreaterThanOrEqual(2);
        const rows = await driver.spaceCommentRows();
        expect(rows.find((row) => row.contentText.includes(marker))!.speakerLabel).toBe("Probe Agent");
        expect(rows.find((row) => row.contentText.includes(humanMarker))!.speakerLabel).toBeNull();
      });

      await test.step("the server agrees with the labels", async () => {
        const comments = await serverSpaceComments(anchor, issueId);
        expect(comments.find((comment) => comment.id === agent.id)).toMatchObject({
          speakerType: "agent",
          speakerLabel: "Probe Agent",
        });
        expect(comments.find((comment) => comment.id === human.id)!.speakerType).toBe("human");
      });
    } finally {
      for (const row of posted) await deleteCommentIfPresent(anchor, row.issueId, row.id, admin);
      await restoreFlags(seed, before, admin);
    }
  }
);

test(
  specTitle(["SPC-053"], "post rich comments through the composer"),
  { tag: specTags(["SPC-053"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[0]!;
    const marker = `rich post ${Date.now()}`;
    const admin = await signInSession(seed.email, seed.password);
    const before = await setFlags(seed, { comments: true, reactions: false, votes: false }, admin);
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);

      await test.step("sign in and open the issue", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenIssue(anchor, issueName);
      });

      await test.step("draft text plus a pasted formatted fragment", async () => {
        await driver.spaceCommentType(marker);
        await driver.spaceCommentPasteHtml("<p>pasted <strong>bold</strong> fragment</p>");
        await expect.poll(() => driver.spaceCommentDraftText(), { timeout: 30_000 }).toContain(marker);
      });

      await test.step("submit posts the comment and clears the composer", async () => {
        await driver.spaceCommentSubmit();
        await expect
          .poll(() => driver.spaceCommentRows().then((rows) => rows.map((row) => row.contentText).join("\n")), {
            timeout: 60_000,
          })
          .toContain(marker);
        await expect.poll(() => driver.spaceCommentDraftText(), { timeout: 30_000 }).toBe("");
        const rows = await driver.spaceCommentRows();
        expect(rows.find((row) => row.contentText.includes(marker))!.contentText).toContain("bold");
      });

      await test.step("the server stored the comment with its formatted body", async () => {
        const comments = await serverSpaceComments(anchor, issueId);
        const stored = comments.find((comment) => comment.stripped.includes(marker))!;
        expect(stored).toBeDefined();
        expect(stored.html).toContain("bold");
      });

      await test.step("clean up the posted comment", async () => {
        const comments = await serverSpaceComments(anchor, issueId);
        const stored = comments.find((comment) => comment.stripped.includes(marker))!;
        await serverSpaceDeleteComment(anchor, issueId, stored.id, admin);
      });
    } finally {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName).catch(() => null);
      if (issueId !== null) {
        const leftovers = (await serverSpaceComments(anchor, issueId).catch(() => [])).filter((comment) =>
          comment.stripped.includes(marker)
        );
        for (const leftover of leftovers) await deleteCommentIfPresent(anchor, issueId, leftover.id, admin);
      }
      await restoreFlags(seed, before, admin);
    }
  }
);

test(
  specTitle(["SPC-053"], "attach an image to a comment"),
  { tag: specTags(["SPC-053"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[0]!;
    const marker = `image attach ${Date.now()}`;
    const admin = await signInSession(seed.email, seed.password);
    const before = await setFlags(seed, { comments: true, reactions: false, votes: false }, admin);
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);

      await test.step("sign in and open the issue", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenIssue(anchor, issueName);
      });

      await test.step("attach an image to the draft and post", async () => {
        await driver.spaceCommentType(marker);
        await driver.spaceCommentAttachImage(uploadFile);
        await driver.spaceCommentSubmit();
        await expect
          .poll(() => driver.spaceCommentRows().then((rows) => rows.map((row) => row.contentText).join("\n")), {
            timeout: 60_000,
          })
          .toContain(marker);
      });

      await test.step("the posted row embeds the image and the server links it", async () => {
        const rows = await driver.spaceCommentRows();
        const row = rows.find((candidate) => candidate.contentText.includes(marker))!;
        expect(row.hasImage).toBe(true);
        const comments = await serverSpaceComments(anchor, issueId);
        const stored = comments.find((comment) => comment.stripped.includes(marker))!;
        expect(stored.html).toMatch(/image-component|<img/);
      });

      await test.step("clean up the posted comment", async () => {
        const comments = await serverSpaceComments(anchor, issueId);
        const stored = comments.find((comment) => comment.stripped.includes(marker))!;
        await serverSpaceDeleteComment(anchor, issueId, stored.id, admin);
      });
    } finally {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName).catch(() => null);
      if (issueId !== null) {
        const leftovers = (await serverSpaceComments(anchor, issueId).catch(() => [])).filter((comment) =>
          comment.stripped.includes(marker)
        );
        for (const leftover of leftovers) await deleteCommentIfPresent(anchor, issueId, leftover.id, admin);
      }
      await restoreFlags(seed, before, admin);
    }
  }
);

test(
  specTitle(["SPC-053"], "failed posts report without losing the thread"),
  { tag: specTags(["SPC-053"]) },
  async ({ driver, seed, page }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[0]!;
    const seededMarker = `seeded thread ${Date.now()}`;
    const failedMarker = `failed post ${Date.now()}`;
    const admin = await signInSession(seed.email, seed.password);
    const before = await setFlags(seed, { comments: true, reactions: false, votes: false }, admin);
    let seededId: string | null = null;
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      seededId = (await serverSpacePostComment(anchor, issueId, `<p>${seededMarker}</p>`, seededMarker, admin)).id;

      await test.step("sign in and open the issue", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenIssue(anchor, issueName);
        await expect
          .poll(() => driver.spaceCommentRows().then((rows) => rows.length), { timeout: 60_000 })
          .toBeGreaterThanOrEqual(1);
      });

      await test.step("a failing post surfaces an error notice", async () => {
        await page.route("**/api/public/anchor/*/issues/*/comments/", (route) => route.abort(), { times: 1 });
        await driver.spaceCommentType(failedMarker);
        await driver.spaceCommentSubmit();
        await expect
          .poll(() => driver.spaceCommentNotices().then((notices) => notices.map((notice) => notice.kind)), {
            timeout: 30_000,
          })
          .toContain("error");
      });

      await test.step("the thread stays intact and the draft is kept", async () => {
        const rows = await driver.spaceCommentRows();
        expect(rows.map((row) => row.contentText).join("\n")).toContain(seededMarker);
        expect(rows.map((row) => row.contentText).join("\n")).not.toContain(failedMarker);
        expect(await driver.spaceCommentDraftText()).toContain(failedMarker);
        const comments = await serverSpaceComments(anchor, issueId);
        expect(comments.map((comment) => comment.stripped)).toContain(seededMarker);
        expect(comments.map((comment) => comment.stripped).join("\n")).not.toContain(failedMarker);
      });
    } finally {
      if (seededId !== null) {
        const issueId = await serverSpaceIssueIdByName(anchor, issueName).catch(() => null);
        if (issueId !== null) await deleteCommentIfPresent(anchor, issueId, seededId, admin);
      }
      await restoreFlags(seed, before, admin);
    }
  }
);

test(
  specTitle(["SPC-054"], "bug: NEWFRONT-460 mention trigger offers no member suggestions"),
  { tag: specTags(["SPC-054"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[0]!;
    const admin = await signInSession(seed.email, seed.password);
    const before = await setFlags(seed, { comments: true, reactions: false, votes: false }, admin);
    try {
      await test.step("the members source is populated", async () => {
        const members = await serverSpaceMembers(anchor);
        expect(members.length).toBeGreaterThan(0);
      });

      await test.step("sign in and open the issue", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenIssue(anchor, issueName);
      });

      await test.step("typing a mention trigger offers nothing", async () => {
        const memberName = seed.mentionMember?.displayName ?? "Parity Mention";
        const query = memberName.slice(0, 3);
        const result = await driver.spaceCommentMention(query, memberName);
        if (driver.target === "web_new") {
          // Intended behavior (bug NEWFRONT-460): the trigger offers
          // matching board members and picking one inserts a chip.
          expect(result.offered).toBe(true);
          expect(result.inserted).toBe(true);
          expect(await driver.spaceCommentDraftText()).toContain(memberName);
          return;
        }
        expect(result.offered).toBe(false);
        expect(result.inserted).toBe(false);
        expect(await driver.spaceCommentMentionOpen()).toBe(false);
        expect(await driver.spaceCommentDraftText()).toContain(`@${query}`);
      });
    } finally {
      await restoreFlags(seed, before, admin);
    }
  }
);

test(
  specTitle(["SPC-055"], "owner menu offers edit and delete on own comments only"),
  { tag: specTags(["SPC-055"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[2]!;
    const ownMarker = `own comment ${Date.now()}`;
    const otherMarker = `other comment ${Date.now()}`;
    const admin = await signInSession(seed.email, seed.password);
    const guestCreds = requireGuest(seed);
    const guest = await signInSession(guestCreds.email, guestCreds.password);
    const before = await setFlags(seed, { comments: true, reactions: false, votes: false }, admin);
    let otherId: string | null = null;
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      await serverSpacePostComment(anchor, issueId, `<p>${ownMarker}</p>`, ownMarker, admin);
      otherId = (await serverSpacePostComment(anchor, issueId, `<p>${otherMarker}</p>`, otherMarker, guest)).id;

      await test.step("sign in and open the issue", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenIssue(anchor, issueName);
        await expect
          .poll(() => driver.spaceCommentRows().then((rows) => rows.length), { timeout: 60_000 })
          .toBeGreaterThanOrEqual(2);
      });

      await test.step("only the own row carries a menu with edit and delete", async () => {
        const rows = await driver.spaceCommentRows();
        const ownIndex = rows.findIndex((row) => row.contentText.includes(ownMarker));
        const otherIndex = rows.findIndex((row) => row.contentText.includes(otherMarker));
        expect(ownIndex).toBeGreaterThanOrEqual(0);
        expect(otherIndex).toBeGreaterThanOrEqual(0);
        expect(await driver.spaceCommentMenuVisible(ownIndex)).toBe(true);
        expect(await driver.spaceCommentMenuVisible(otherIndex)).toBe(false);
        expect(await driver.spaceCommentMenuItems(ownIndex)).toEqual(["Edit", "Delete"]);
      });

      await test.step("delete removes the own comment", async () => {
        const rows = await driver.spaceCommentRows();
        const ownIndex = rows.findIndex((row) => row.contentText.includes(ownMarker));
        await driver.spaceCommentDelete(ownIndex);
        await expect
          .poll(
            () => driver.spaceCommentRows().then((candidates) => candidates.map((row) => row.contentText).join("\n")),
            {
              timeout: 30_000,
            }
          )
          .not.toContain(ownMarker);
        const comments = await serverSpaceComments(anchor, issueId);
        expect(comments.map((comment) => comment.stripped).join("\n")).not.toContain(ownMarker);
        expect(comments.map((comment) => comment.stripped)).toContain(otherMarker);
      });

      await test.step("embedded frames show no menu at all", async () => {
        await driver.spaceCommentOpenEmbed(anchor, issueName);
        const section = await driver.spaceCommentEmbedSection();
        expect(section.threadCount).toBeGreaterThanOrEqual(1);
        expect(await driver.spaceCommentEmbedMenuVisible(0)).toBe(false);
      });
    } finally {
      if (otherId !== null) {
        const issueId = await serverSpaceIssueIdByName(anchor, issueName).catch(() => null);
        if (issueId !== null) await deleteCommentIfPresent(anchor, issueId, otherId, guest);
      }
      const issueId = await serverSpaceIssueIdByName(anchor, issueName).catch(() => null);
      if (issueId !== null) {
        const leftovers = (await serverSpaceComments(anchor, issueId).catch(() => [])).filter((comment) =>
          comment.stripped.includes(ownMarker)
        );
        for (const leftover of leftovers) await deleteCommentIfPresent(anchor, issueId, leftover.id, admin);
      }
      await restoreFlags(seed, before, admin);
    }
  }
);

test(
  specTitle(["SPC-056"], "comment edits apply instantly"),
  { tag: specTags(["SPC-056"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[2]!;
    const marker = `edit before ${Date.now()}`;
    const edited = `edit after ${Date.now()}`;
    const admin = await signInSession(seed.email, seed.password);
    const before = await setFlags(seed, { comments: true, reactions: false, votes: false }, admin);
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      const posted = await serverSpacePostComment(anchor, issueId, `<p>${marker}</p>`, marker, admin);

      await test.step("sign in and open the issue", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenIssue(anchor, issueName);
        await expect
          .poll(() => driver.spaceCommentRows().then((rows) => rows.length), { timeout: 60_000 })
          .toBeGreaterThanOrEqual(1);
      });

      await test.step("saving swaps the content at once", async () => {
        const rows = await driver.spaceCommentRows();
        const index = rows.findIndex((row) => row.contentText.includes(marker));
        expect(index).toBeGreaterThanOrEqual(0);
        await driver.spaceCommentEdit(index, edited);
        await expect
          .poll(
            () => driver.spaceCommentRows().then((candidates) => candidates.map((row) => row.contentText).join("\n")),
            {
              timeout: 30_000,
            }
          )
          .toContain(edited);
      });

      await test.step("the server stored the edit", async () => {
        const comments = await serverSpaceComments(anchor, issueId);
        expect(comments.find((comment) => comment.id === posted.id)!.html).toContain(edited);
        await serverSpaceDeleteComment(anchor, issueId, posted.id, admin);
      });
    } finally {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName).catch(() => null);
      if (issueId !== null) {
        const leftovers = (await serverSpaceComments(anchor, issueId).catch(() => [])).filter(
          (comment) => comment.stripped.includes(marker) || comment.stripped.includes(edited)
        );
        for (const leftover of leftovers) await deleteCommentIfPresent(anchor, issueId, leftover.id, admin);
      }
      await restoreFlags(seed, before, admin);
    }
  }
);

test(
  specTitle(["SPC-056"], "bug: NEWFRONT-467 failed saves leave the phantom edit on screen"),
  { tag: specTags(["SPC-056"]) },
  async ({ driver, seed, page }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[2]!;
    const marker = `reconcile before ${Date.now()}`;
    const attempted = `reconcile after ${Date.now()}`;
    const admin = await signInSession(seed.email, seed.password);
    const before = await setFlags(seed, { comments: true, reactions: false, votes: false }, admin);
    let commentId: string | null = null;
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      commentId = (await serverSpacePostComment(anchor, issueId, `<p>${marker}</p>`, marker, admin)).id;

      await test.step("sign in and open the issue", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenIssue(anchor, issueName);
        await expect
          .poll(() => driver.spaceCommentRows().then((rows) => rows.length), { timeout: 60_000 })
          .toBeGreaterThanOrEqual(1);
      });

      await test.step("a failing save swaps to the attempt and never swaps back", async () => {
        await page.route(
          "**/api/public/anchor/*/issues/*/comments/*/",
          async (route) => {
            await new Promise((resolve) => setTimeout(resolve, 2_000));
            await route.abort();
          },
          { times: 1 }
        );
        const bodies = () =>
          driver.spaceCommentRows().then((candidates) => candidates.map((row) => row.contentText).join("\n"));
        const rows = await driver.spaceCommentRows();
        const index = rows.findIndex((row) => row.contentText.includes(marker));
        expect(index).toBeGreaterThanOrEqual(0);
        await driver.spaceCommentEdit(index, attempted);
        // Intended on web_new (bug NEWFRONT-467 fixed): a failed save
        // reconciles the row back to the stored marker instead of leaving
        // the attempted text on screen.
        if (driver.target === "web_new") {
          await expect.poll(bodies, { timeout: 30_000 }).toContain(marker);
          await driver.page.waitForTimeout(10_000);
          expect(await bodies()).toContain(marker);
          expect(await bodies()).not.toContain(attempted);
          return;
        }
        await expect.poll(bodies, { timeout: 30_000 }).toContain(attempted);
        // Pinned: the phantom edit stays on screen (the display editor
        // keeps the attempted text even though the store re-listed).
        await driver.page.waitForTimeout(10_000);
        expect(await bodies()).toContain(attempted);
        expect(await bodies()).not.toContain(marker);
      });

      await test.step("the server never stored the phantom edit", async () => {
        const comments = await serverSpaceComments(anchor, issueId);
        const stored = comments.find((comment) => comment.id === commentId)!;
        expect(stored.html).toContain(marker);
        expect(stored.html).not.toContain(attempted);
      });
    } finally {
      if (commentId !== null) {
        const issueId = await serverSpaceIssueIdByName(anchor, issueName).catch(() => null);
        if (issueId !== null) await deleteCommentIfPresent(anchor, issueId, commentId, admin);
      }
      await restoreFlags(seed, before, admin);
    }
  }
);

test(
  specTitle(["SPC-057"], "bug: NEWFRONT-466 comment reaction pills omit reactor names"),
  { tag: specTags(["SPC-057"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[2]!;
    const marker = `comment react names ${Date.now()}`;
    const admin = await signInSession(seed.email, seed.password);
    const guestCreds = requireGuest(seed);
    const guest = await signInSession(guestCreds.email, guestCreds.password);
    const before = await setFlags(seed, { comments: true, reactions: true, votes: false }, admin);
    const partyCode = "127881";
    const partyEmoji = "🎉";
    let commentId: string | null = null;
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      commentId = (await serverSpacePostComment(anchor, issueId, `<p>${marker}</p>`, marker, admin)).id;
      await serverSpaceCommentReact(anchor, commentId, partyCode, guest);
      const guestName = (await serverSpaceComments(anchor, issueId)).find((comment) => comment.id === commentId)!
        .reactions[0]!.displayName;
      expect(guestName).not.toBe("");

      await test.step("sign in and open the issue", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenIssue(anchor, issueName);
        await expect
          .poll(() => driver.spaceCommentRows().then((rows) => rows.length), { timeout: 60_000 })
          .toBeGreaterThanOrEqual(1);
      });

      await test.step("the pill groups the count but names nobody", async () => {
        const rows = await driver.spaceCommentRows();
        const index = rows.findIndex((row) => row.contentText.includes(marker));
        expect(index).toBeGreaterThanOrEqual(0);
        // Pinned: the pill carries the count while the reactor names stay
        // empty (the view reads a nested actor the payload never sends).
        // Intended on web_new (bug NEWFRONT-466 fixed): the pill names its
        // reactor from the flat display name the payload sends.
        if (driver.target === "web_new") {
          expect(await driver.spaceCommentReactions(index)).toEqual([
            { emoji: partyEmoji, count: 1, mine: false, users: [guestName] },
          ]);
        } else {
          expect(await driver.spaceCommentReactions(index)).toEqual([
            { emoji: partyEmoji, count: 1, mine: false, users: [] },
          ]);
        }
        expect(await driver.spaceCommentReactAddVisible(index)).toBe(true);
      });
    } finally {
      if (commentId !== null) {
        const issueId = await serverSpaceIssueIdByName(anchor, issueName).catch(() => null);
        if (issueId !== null) await deleteCommentIfPresent(anchor, issueId, commentId, admin);
      }
      await restoreFlags(seed, before, admin);
    }
  }
);

test(
  specTitle(["SPC-057"], "comment reactions toggle and add through the picker"),
  { tag: specTags(["SPC-057"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[2]!;
    const marker = `comment react ${Date.now()}`;
    const admin = await signInSession(seed.email, seed.password);
    const guestCreds = requireGuest(seed);
    const guest = await signInSession(guestCreds.email, guestCreds.password);
    const before = await setFlags(seed, { comments: true, reactions: true, votes: false }, admin);
    const partyCode = "127881";
    const partyEmoji = "🎉";
    let commentId: string | null = null;
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      commentId = (await serverSpacePostComment(anchor, issueId, `<p>${marker}</p>`, marker, admin)).id;
      await serverSpaceCommentReact(anchor, commentId, partyCode, guest);
      const guestName = (await serverSpaceComments(anchor, issueId)).find((comment) => comment.id === commentId)!
        .reactions[0]!.displayName;
      const adminName = (await serverSpaceComments(anchor, issueId)).find(
        (comment) => comment.id === commentId
      )!.displayName;

      await test.step("sign in and open the issue", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenIssue(anchor, issueName);
        await expect
          .poll(() => driver.spaceCommentRows().then((rows) => rows.length), { timeout: 60_000 })
          .toBeGreaterThanOrEqual(1);
      });

      const rowIndex = async (): Promise<number> => {
        const rows = await driver.spaceCommentRows();
        const index = rows.findIndex((row) => row.contentText.includes(marker));
        expect(index).toBeGreaterThanOrEqual(0);
        return index;
      };

      await test.step("the guest pill groups the count with the add control beside it", async () => {
        // Counts only here: the missing reactor names are pinned by the
        // NEWFRONT-466 scenario, not this one.
        const groups = await driver.spaceCommentReactions(await rowIndex());
        expect(groups).toMatchObject([{ emoji: partyEmoji, count: 1, mine: false }]);
        expect(await driver.spaceCommentReactAddVisible(await rowIndex())).toBe(true);
      });

      await test.step("clicking the pill adds the visitor, re-clicking removes them", async () => {
        // The screen swaps optimistically, so each toggle settles
        // server-side before the next one fires; otherwise the writes race.
        const storedReactions = async () =>
          (await serverSpaceComments(anchor, issueId)).find((comment) => comment.id === commentId)!.reactions;
        const index = await rowIndex();
        await driver.spaceCommentReactClick(index, partyEmoji);
        await expect.poll(() => storedReactions().then((rows) => rows.length), { timeout: 60_000 }).toBe(2);
        const grouped = await driver.spaceCommentReactions(index);
        expect(grouped).toMatchObject([{ emoji: partyEmoji, count: 2, mine: true }]);
        expect(grouped[0]!.users).toContain(adminName);
        await driver.spaceCommentReactClick(index, partyEmoji);
        await expect.poll(() => storedReactions().then((rows) => rows.length), { timeout: 60_000 }).toBe(1);
      });

      await test.step("the picker adds a reaction that the pill then removes", async () => {
        const storedReactions = async () =>
          (await serverSpaceComments(anchor, issueId)).find((comment) => comment.id === commentId)!.reactions;
        const pickedActors = async (code: string) =>
          (await storedReactions()).filter((row) => row.reaction === code).map((row) => row.displayName);
        const index = await rowIndex();
        const picked = await driver.spaceCommentReactPick(index);
        await expect.poll(() => pickedActors(picked.code), { timeout: 60_000 }).toContain(adminName);
        await expect
          .poll(
            () =>
              driver
                .spaceCommentReactions(index)
                .then((groups) => groups.find((group) => group.emoji === picked.emoji)),
            { timeout: 30_000 }
          )
          .toMatchObject({ mine: true });
        await driver.spaceCommentReactClick(index, picked.emoji);
        await expect.poll(() => pickedActors(picked.code), { timeout: 60_000 }).not.toContain(adminName);
      });

      await test.step("the server agrees with the screen", async () => {
        const stored = (await serverSpaceComments(anchor, issueId)).find((comment) => comment.id === commentId)!;
        expect(stored.reactions).toEqual([expect.objectContaining({ reaction: partyCode, displayName: guestName })]);
      });
    } finally {
      if (commentId !== null) {
        const issueId = await serverSpaceIssueIdByName(anchor, issueName).catch(() => null);
        if (issueId !== null) await deleteCommentIfPresent(anchor, issueId, commentId, admin);
      }
      await restoreFlags(seed, before, admin);
    }
  }
);

test(
  specTitle(["SPC-057"], "bug: NEWFRONT-465 anonymous comment reactions lose the board view on the way to sign-in"),
  { tag: specTags(["SPC-057"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[2]!;
    const marker = `anon comment react ${Date.now()}`;
    const admin = await signInSession(seed.email, seed.password);
    const guestCreds = requireGuest(seed);
    const guest = await signInSession(guestCreds.email, guestCreds.password);
    const before = await setFlags(seed, { comments: true, reactions: true, votes: false }, admin);
    const partyCode = "127881";
    const partyEmoji = "🎉";
    let commentId: string | null = null;
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      commentId = (await serverSpacePostComment(anchor, issueId, `<p>${marker}</p>`, marker, admin)).id;
      await serverSpaceCommentReact(anchor, commentId, partyCode, guest);
      const reactionsBefore = (await serverSpaceComments(anchor, issueId)).find(
        (comment) => comment.id === commentId
      )!.reactions;

      await test.step("open the issue signed out from the kanban board", async () => {
        await driver.spaceCommentOpenIssue(anchor, issueName, "kanban");
        await expect
          .poll(() => driver.spaceCommentRows().then((rows) => rows.length), { timeout: 60_000 })
          .toBeGreaterThanOrEqual(1);
      });

      await test.step("reacting lands on the auth gate keeping the issue but dropping the view", async () => {
        const rows = await driver.spaceCommentRows();
        const index = rows.findIndex((row) => row.contentText.includes(marker));
        expect(index).toBeGreaterThanOrEqual(0);
        await driver.spaceCommentReactClick(index, partyEmoji);
        await expect
          .poll(() => driver.spaceCommentAuthLanding().then((landing) => landing.onAuthGate), {
            timeout: 30_000,
          })
          .toBe(true);
        const landing = await driver.spaceCommentAuthLanding();
        expect(landing.returnAnchor).toBe(anchor);
        expect(landing.returnIssueId).toBe(issueId);
        // Intended on web_new (bug NEWFRONT-465 fixed): the board view
        // rides along to sign-in instead of being dropped.
        if (driver.target === "web_new") {
          expect(landing.returnView).toBe("kanban");
        } else {
          expect(landing.returnView).toBeNull();
        }
      });

      await test.step("no reaction was stored", async () => {
        const stored = (await serverSpaceComments(anchor, issueId)).find((comment) => comment.id === commentId)!;
        expect(stored.reactions).toEqual(reactionsBefore);
      });
    } finally {
      if (commentId !== null) {
        const issueId = await serverSpaceIssueIdByName(anchor, issueName).catch(() => null);
        if (issueId !== null) await deleteCommentIfPresent(anchor, issueId, commentId, admin);
      }
      await restoreFlags(seed, before, admin);
    }
  }
);

test(
  specTitle(["SPC-057"], "embedded comments show reaction counts with no add control"),
  { tag: specTags(["SPC-057"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[2]!;
    const marker = `embed comment react ${Date.now()}`;
    const admin = await signInSession(seed.email, seed.password);
    const guestCreds = requireGuest(seed);
    const guest = await signInSession(guestCreds.email, guestCreds.password);
    const before = await setFlags(seed, { comments: true, reactions: true, votes: false }, admin);
    const partyCode = "127881";
    const partyEmoji = "🎉";
    let commentId: string | null = null;
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      commentId = (await serverSpacePostComment(anchor, issueId, `<p>${marker}</p>`, marker, admin)).id;
      await serverSpaceCommentReact(anchor, commentId, partyCode, guest);

      await test.step("open the issue inside an embedded frame while signed in", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenEmbed(anchor, issueName);
      });

      await test.step("counts render with no add control", async () => {
        const section = await driver.spaceCommentEmbedSection();
        expect(section.threadCount).toBeGreaterThanOrEqual(1);
        const facts = await driver.spaceCommentEmbedReactions(0);
        expect(facts.groups).toEqual([{ emoji: partyEmoji, count: 1, mine: false, users: [] }]);
        expect(facts.addVisible).toBe(false);
      });
    } finally {
      if (commentId !== null) {
        const issueId = await serverSpaceIssueIdByName(anchor, issueName).catch(() => null);
        if (issueId !== null) await deleteCommentIfPresent(anchor, issueId, commentId, admin);
      }
      await restoreFlags(seed, before, admin);
    }
  }
);
