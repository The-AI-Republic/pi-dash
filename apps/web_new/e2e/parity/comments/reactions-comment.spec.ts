// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-116): react to an individual comment with an
// emoji — same toggle/count/who-reacted behavior as the work-item cluster —
// and the cluster hides entirely where commenting is disabled (archived
// detail). Row: CMT-023.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverAddCommentReactionRaw,
  serverArchiveIssue,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCommentReactions,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverCreateState,
  serverDeleteState,
  serverIssue,
  serverPatchIssue,
  serverPostComment,
  serverProjectStates,
  serverUnarchiveIssue,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

// Cold dev servers compile route bundles on first load, which can take
// minutes on a loaded host; each step still carries its own poll timeout.
test.setTimeout(600_000);

test(
  specTitle(["CMT-023"], "react to a comment with toggle, count and reactor names"),
  { tag: specTags(["CMT-023"]) },
  async ({ driver, seed }) => {
    const tag = `NF116 comment ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      const body = `${tag} body`;
      const posted = await serverPostComment(seed.workspaceSlug, seed.projectId, issue.id, `<p>${body}</p>`, session);
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openIssueDetail(seed.workspaceSlug, seed.projectId, issue.id);

      await test.step("picker add shows a highlighted chip with count and server state", async () => {
        expect(await driver.activityCommentReactionChips(body)).toEqual([]);
        const picked = await driver.activityAddCommentReaction(body);
        await expect
          .poll(() => driver.activityCommentReactionChips(body), { timeout: 15_000 })
          .toEqual([{ emoji: picked.emoji, count: 1, reacted: true }]);
        // Notices first: success toasts dismiss within seconds, so the
        // server read below would outlive them.
        await expect.poll(() => driver.sawToast("Reaction created successfully"), { timeout: 15_000 }).toBe(true);
        const reactions = await serverCommentReactions(seed.workspaceSlug, seed.projectId, posted.id, session);
        expect(reactions).toHaveLength(1);
        expect(reactions[0]!.reaction).toBe(picked.code);
      });

      await test.step("hovering the chip names who reacted", async () => {
        const picked = (await driver.activityCommentReactionChips(body))[0]!;
        const tooltip = await driver.activityChipTooltipText(body, picked.emoji, "Parity Oracle");
        expect(tooltip).toContain("Parity Oracle");
      });

      await test.step("clicking the chip again removes the reaction", async () => {
        const picked = (await driver.activityCommentReactionChips(body))[0]!;
        await driver.activityClickCommentReactionChip(body, picked.emoji);
        await expect.poll(() => driver.activityCommentReactionChips(body), { timeout: 15_000 }).toEqual([]);
        await expect
          .poll(() => serverCommentReactions(seed.workspaceSlug, seed.projectId, posted.id, session), {
            timeout: 15_000,
          })
          .toEqual([]);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);

test(
  specTitle(["CMT-023"], "comment reaction cluster hides where commenting is disabled"),
  { tag: specTags(["CMT-023"]) },
  async ({ driver, seed }) => {
    const tag = `NF116 hidden ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N116"),
      {},
      session
    );
    // Archiving requires a completed or cancelled state; reuse a
    // project-default one when present, else mint a scenario-owned one.
    const states = await serverProjectStates(seed.workspaceSlug, projectId, session);
    const seededDone = states.find((s) => s.group === "completed" || s.group === "cancelled");
    const ownedStateId =
      seededDone === undefined
        ? await serverCreateState(seed.workspaceSlug, projectId, `${tag} done`, "completed", session)
        : null;
    const doneStateId = seededDone?.id ?? (ownedStateId as string);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    const body = `${tag} body`;
    const posted = await serverPostComment(seed.workspaceSlug, projectId, issue.id, `<p>${body}</p>`, session);
    // Seed a reaction while the issue is still open, so the archived view
    // proves the cluster hides even when reactions exist server-side.
    const seeded = await serverAddCommentReactionRaw(seed.workspaceSlug, projectId, posted.id, session, "128077");
    expect(seeded.status).toBe(201);
    await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { state_id: doneStateId }, session);
    await serverArchiveIssue(seed.workspaceSlug, projectId, issue.id, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.openArchivedIssueDetail(seed.workspaceSlug, projectId, issue.id);

      await test.step("the server marks the issue archived with the reaction stored", async () => {
        const detail = await serverIssue(seed.workspaceSlug, projectId, issue.id, session);
        expect(detail.archived_at).not.toBe(null);
        expect(await serverCommentReactions(seed.workspaceSlug, projectId, posted.id, session)).toHaveLength(1);
      });

      await test.step("no reaction UI renders on the read-only cards", async () => {
        // The card itself still renders its body; only the reaction UI goes.
        await expect.poll(() => driver.activityCommentBodyVisible(body), { timeout: 30_000 }).toBe(true);
        expect(await driver.reactionsArchivedCommentClusterVisible(body)).toBe(false);
        expect(await driver.archivesDetailReactionControlEnabled()).toBe(false);
        expect(await driver.archivesDetailComposerVisible()).toBe(false);
      });
    } finally {
      await serverUnarchiveIssue(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      if (ownedStateId !== null)
        await serverDeleteState(seed.workspaceSlug, projectId, ownedStateId, session).catch(() => {});
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
