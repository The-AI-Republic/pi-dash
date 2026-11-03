// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-451): public-board issue engagement — votes
// (SPC-047) and emoji reactions (SPC-048). Each scenario flips the board
// engagement flags through the publish API and restores them, so reruns
// on one seeded stack stay green; votes and reactions assert against the
// embedded issue-detail items (the public votes/reactions list endpoints
// resolve the board by the wrong key and always answer [], observed live).
// The two anonymous-redirect scenarios pin NEWFRONT-465 (the return
// address drops the board view); the rows record the intent.
import { test, expect } from "../fixtures";
import {
  serverSpaceDeleteReaction,
  serverSpaceDeleteVote,
  serverSpaceIssueEngagement,
  serverSpaceIssueIdByName,
  serverSpaceReact,
  serverSpaceBoardFlags,
  serverSpaceSetBoardFlags,
  serverSpaceVote,
  sessionBrowserCookies,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParitySeedFacts } from "../drivers/parity-driver";

// Cold dev servers compile route bundles on first load, which can take
// minutes on a loaded host; each step still carries its own poll timeout.
test.setTimeout(600_000);

function requireAnchor(seed: ParitySeedFacts): string {
  if (!seed.spaceAnchor) throw new Error("[parity] seed facts carry no spaceAnchor; re-run the stack seed step.");
  return seed.spaceAnchor;
}

function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.guestEmail || !seed.guestPassword)
    throw new Error("[parity] seed facts carry no guest identity; re-run the stack seed step.");
  return { email: seed.guestEmail, password: seed.guestPassword };
}

async function setBoardFlags(
  workspaceSlug: string,
  projectId: string,
  flags: { comments: boolean; reactions: boolean; votes: boolean },
  session: string
) {
  const before = await serverSpaceBoardFlags(workspaceSlug, projectId, session);
  await serverSpaceSetBoardFlags(workspaceSlug, projectId, session, flags);
  return before;
}

/** Delete a vote only when one exists (a missing row answers 500, not 404). */
async function deleteVoteIfPresent(anchor: string, issueId: string, session: string): Promise<void> {
  try {
    await serverSpaceDeleteVote(anchor, issueId, session);
  } catch (error) {
    if (!/HTTP (404|500)/.test(error instanceof Error ? error.message : "")) throw error;
  }
}

/** Delete a reaction only when one exists (a missing row answers 500, not 404). */
async function deleteReactionIfPresent(
  anchor: string,
  issueId: string,
  reaction: string,
  session: string
): Promise<void> {
  try {
    await serverSpaceDeleteReaction(anchor, issueId, reaction, session);
  } catch (error) {
    if (!/HTTP (404|500)/.test(error instanceof Error ? error.message : "")) throw error;
  }
}

test(
  specTitle(["SPC-047"], "votes toggle with counts, voter names and own-vote state"),
  { tag: specTags(["SPC-047"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[0]!;
    const admin = await signInSession(seed.email, seed.password);
    const guestCreds = requireGuest(seed);
    const guest = await signInSession(guestCreds.email, guestCreds.password);
    const before = await setBoardFlags(
      seed.workspaceSlug,
      seed.projectId,
      { comments: false, reactions: false, votes: true },
      admin
    );
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      await test.step("converge to a known state: one guest up-vote", async () => {
        await deleteVoteIfPresent(anchor, issueId, admin);
        await deleteVoteIfPresent(anchor, issueId, guest);
        await serverSpaceVote(anchor, issueId, 1, guest);
      });
      const guestName = (await serverSpaceIssueEngagement(anchor, issueId)).votes[0]!.displayName;

      await test.step("sign in and open the issue", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenIssue(anchor, issueName);
      });

      await test.step("the guest vote shows with its voter name and no own-vote mark", async () => {
        const state = await driver.spaceVoteOverlay();
        expect(state.up.count).toBe(1);
        expect(state.up.mine).toBe(false);
        expect(state.up.voters).toEqual([guestName]);
        expect(state.down.count).toBe(0);
        expect(state.down.mine).toBe(false);
        expect(state.down.voters).toEqual([]);
      });

      await test.step("voting up adds the visitor with both names shown", async () => {
        await driver.spaceVoteClick("up");
        await expect.poll(() => driver.spaceVoteOverlay().then((state) => state.up.count), { timeout: 30_000 }).toBe(2);
        const state = await driver.spaceVoteOverlay();
        expect(state.up.mine).toBe(true);
        expect(state.up.voters).toEqual(expect.arrayContaining([guestName]));
        expect(state.up.voters).toHaveLength(2);
      });

      await test.step("re-clicking removes the visitor's own vote", async () => {
        await driver.spaceVoteClick("up");
        await expect.poll(() => driver.spaceVoteOverlay().then((state) => state.up.count), { timeout: 30_000 }).toBe(1);
        const state = await driver.spaceVoteOverlay();
        expect(state.up.mine).toBe(false);
        expect(state.up.voters).toEqual([guestName]);
      });

      await test.step("switching sides replaces the vote instead of stacking", async () => {
        await driver.spaceVoteClick("up");
        await expect.poll(() => driver.spaceVoteOverlay().then((state) => state.up.count), { timeout: 30_000 }).toBe(2);
        await driver.spaceVoteClick("down");
        await expect
          .poll(() => driver.spaceVoteOverlay().then((state) => state.down.count), { timeout: 30_000 })
          .toBe(1);
        const state = await driver.spaceVoteOverlay();
        expect(state.up.count).toBe(1);
        expect(state.up.mine).toBe(false);
        expect(state.down.count).toBe(1);
        expect(state.down.mine).toBe(true);
        await driver.spaceVoteClick("down");
        await expect
          .poll(() => driver.spaceVoteOverlay().then((state) => state.down.count), { timeout: 30_000 })
          .toBe(0);
      });

      await test.step("the server agrees with the screen", async () => {
        const engagement = await serverSpaceIssueEngagement(anchor, issueId);
        expect(engagement.votes).toHaveLength(1);
        expect(engagement.votes[0]).toMatchObject({ vote: 1, displayName: guestName });
        const state = await driver.spaceVoteOverlay();
        expect(state.up.voters).toEqual([guestName]);
      });

      await test.step("clean up the guest vote", async () => {
        await deleteVoteIfPresent(anchor, issueId, guest);
        expect((await serverSpaceIssueEngagement(anchor, issueId)).votes).toHaveLength(0);
      });
    } finally {
      await setBoardFlags(
        seed.workspaceSlug,
        seed.projectId,
        { comments: before.comments, reactions: before.reactions, votes: before.votes },
        admin
      );
    }
  }
);

test(
  specTitle(["SPC-047"], "vote controls hide when the publisher disables votes"),
  { tag: specTags(["SPC-047"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[0]!;
    const admin = await signInSession(seed.email, seed.password);
    const before = await setBoardFlags(
      seed.workspaceSlug,
      seed.projectId,
      { comments: false, reactions: false, votes: false },
      admin
    );
    try {
      await test.step("sign in and open the issue", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenIssue(anchor, issueName);
      });

      await test.step("the overlay renders no vote controls", async () => {
        expect(await driver.spaceVoteOverlayVisible()).toBe(false);
      });

      await test.step("kanban cards render no vote footer", async () => {
        expect(await driver.spaceVoteCardVisible(anchor, issueName)).toBe(false);
      });
    } finally {
      await setBoardFlags(
        seed.workspaceSlug,
        seed.projectId,
        { comments: before.comments, reactions: before.reactions, votes: before.votes },
        admin
      );
    }
  }
);

test(
  specTitle(["SPC-047"], "bug: NEWFRONT-465 anonymous votes lose the board view on the way to sign-in"),
  { tag: specTags(["SPC-047"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[0]!;
    const admin = await signInSession(seed.email, seed.password);
    const before = await setBoardFlags(
      seed.workspaceSlug,
      seed.projectId,
      { comments: false, reactions: false, votes: true },
      admin
    );
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      const votesBefore = (await serverSpaceIssueEngagement(anchor, issueId)).votes;

      await test.step("open the issue signed out from the kanban board", async () => {
        await driver.spaceCommentOpenIssue(anchor, issueName, "kanban");
        expect(await driver.spaceVoteOverlayVisible()).toBe(true);
      });

      await test.step("voting lands on the auth gate keeping the issue but dropping the view", async () => {
        await driver.spaceVoteClick("up");
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

      await test.step("no vote was stored", async () => {
        expect((await serverSpaceIssueEngagement(anchor, issueId)).votes).toEqual(votesBefore);
      });
    } finally {
      await setBoardFlags(
        seed.workspaceSlug,
        seed.projectId,
        { comments: before.comments, reactions: before.reactions, votes: before.votes },
        admin
      );
    }
  }
);

test(
  specTitle(["SPC-047"], "kanban card footers vote and toggle inline"),
  { tag: specTags(["SPC-047"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[1]!;
    const admin = await signInSession(seed.email, seed.password);
    const before = await setBoardFlags(
      seed.workspaceSlug,
      seed.projectId,
      { comments: false, reactions: false, votes: true },
      admin
    );
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      await deleteVoteIfPresent(anchor, issueId, admin);

      await test.step("sign in; the card footer starts empty", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        const state = await driver.spaceVoteCard(anchor, issueName);
        expect(state.up.count).toBe(0);
        expect(state.up.mine).toBe(false);
        expect(state.down.count).toBe(0);
      });

      await test.step("voting from the card toggles the count and the own-vote mark", async () => {
        await driver.spaceVoteCardClick(anchor, issueName, "up");
        await expect
          .poll(() => driver.spaceVoteCard(anchor, issueName).then((state) => state.up.count), { timeout: 60_000 })
          .toBe(1);
        expect((await driver.spaceVoteCard(anchor, issueName)).up.mine).toBe(true);
        await driver.spaceVoteCardClick(anchor, issueName, "up");
        await expect
          .poll(() => driver.spaceVoteCard(anchor, issueName).then((state) => state.up.count), { timeout: 60_000 })
          .toBe(0);
        expect((await driver.spaceVoteCard(anchor, issueName)).up.mine).toBe(false);
      });

      await test.step("the server agrees with the card", async () => {
        expect((await serverSpaceIssueEngagement(anchor, issueId)).votes).toHaveLength(0);
      });
    } finally {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName).catch(() => null);
      if (issueId !== null) await deleteVoteIfPresent(anchor, issueId, admin);
      await setBoardFlags(
        seed.workspaceSlug,
        seed.projectId,
        { comments: before.comments, reactions: before.reactions, votes: before.votes },
        admin
      );
    }
  }
);

test(
  specTitle(["SPC-048"], "reactions group with counts, add through the picker, re-click removes"),
  { tag: specTags(["SPC-048"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[2]!;
    const admin = await signInSession(seed.email, seed.password);
    const guestCreds = requireGuest(seed);
    const guest = await signInSession(guestCreds.email, guestCreds.password);
    const before = await setBoardFlags(
      seed.workspaceSlug,
      seed.projectId,
      { comments: false, reactions: true, votes: false },
      admin
    );
    // Party popper in the decimal code-point form the API stores.
    const partyCode = "127881";
    const partyEmoji = "🎉";
    let adminName = "";
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      await test.step("converge to a known state: one guest reaction", async () => {
        const engagement = await serverSpaceIssueEngagement(anchor, issueId);
        for (const reaction of new Set(engagement.reactions.map((row) => row.reaction))) {
          await deleteReactionIfPresent(anchor, issueId, reaction, admin);
          await deleteReactionIfPresent(anchor, issueId, reaction, guest);
        }
        await serverSpaceReact(anchor, issueId, partyCode, guest);
      });
      const guestName = (await serverSpaceIssueEngagement(anchor, issueId)).reactions[0]!.displayName;

      await test.step("sign in and open the issue", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenIssue(anchor, issueName);
        expect(await driver.spaceReactOverlayVisible()).toBe(true);
        expect(await driver.spaceReactAddVisible()).toBe(true);
      });

      await test.step("the guest reaction shows grouped with its reactor", async () => {
        expect(await driver.spaceReactGroups()).toEqual([
          { emoji: partyEmoji, count: 1, mine: false, users: [guestName] },
        ]);
      });

      await test.step("clicking the pill adds the visitor, re-clicking removes them", async () => {
        // The screen swaps optimistically, so each toggle settles
        // server-side before the next one fires; otherwise the writes race.
        // Settles key on the visitor's own row: the guest row pre-exists.
        await driver.spaceReactClickGroup(partyEmoji);
        await expect
          .poll(() => serverSpaceIssueEngagement(anchor, issueId).then((state) => state.reactions.length), {
            timeout: 60_000,
          })
          .toBe(2);
        adminName = (await serverSpaceIssueEngagement(anchor, issueId)).reactions.find(
          (row) => row.displayName !== guestName
        )!.displayName;
        expect(await driver.spaceReactGroups()).toEqual([
          { emoji: partyEmoji, count: 2, mine: true, users: expect.arrayContaining([guestName]) },
        ]);
        await driver.spaceReactClickGroup(partyEmoji);
        await expect
          .poll(() => serverSpaceIssueEngagement(anchor, issueId).then((state) => state.reactions.length), {
            timeout: 60_000,
          })
          .toBe(1);
        expect(await driver.spaceReactGroups()).toEqual([
          { emoji: partyEmoji, count: 1, mine: false, users: [guestName] },
        ]);
      });

      await test.step("the picker adds a reaction that the pill then removes", async () => {
        const picked = await driver.spaceReactPick();
        const pickedActors = () =>
          serverSpaceIssueEngagement(anchor, issueId).then((state) =>
            state.reactions.filter((row) => row.reaction === picked.code).map((row) => row.displayName)
          );
        await expect.poll(pickedActors, { timeout: 60_000 }).toContain(adminName);
        await expect
          .poll(
            () => driver.spaceReactGroups().then((groups) => groups.find((group) => group.emoji === picked.emoji)),
            { timeout: 30_000 }
          )
          .toMatchObject({ mine: true });
        await driver.spaceReactClickGroup(picked.emoji);
        await expect.poll(pickedActors, { timeout: 60_000 }).not.toContain(adminName);
      });

      await test.step("the server agrees with the screen", async () => {
        const engagement = await serverSpaceIssueEngagement(anchor, issueId);
        expect(engagement.reactions).toEqual([
          expect.objectContaining({ reaction: partyCode, displayName: guestName }),
        ]);
      });

      await test.step("clean up the guest reaction", async () => {
        await deleteReactionIfPresent(anchor, issueId, partyCode, guest);
        expect((await serverSpaceIssueEngagement(anchor, issueId)).reactions).toHaveLength(0);
      });
    } finally {
      await setBoardFlags(
        seed.workspaceSlug,
        seed.projectId,
        { comments: before.comments, reactions: before.reactions, votes: before.votes },
        admin
      );
    }
  }
);

test(
  specTitle(["SPC-048"], "reaction controls hide when the publisher disables reactions"),
  { tag: specTags(["SPC-048"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[2]!;
    const admin = await signInSession(seed.email, seed.password);
    const before = await setBoardFlags(
      seed.workspaceSlug,
      seed.projectId,
      { comments: false, reactions: false, votes: false },
      admin
    );
    try {
      await test.step("sign in and open the issue", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenIssue(anchor, issueName);
      });

      await test.step("the overlay renders no reaction control", async () => {
        expect(await driver.spaceReactOverlayVisible()).toBe(false);
        expect(await driver.spaceReactAddVisible()).toBe(false);
      });
    } finally {
      await setBoardFlags(
        seed.workspaceSlug,
        seed.projectId,
        { comments: before.comments, reactions: before.reactions, votes: before.votes },
        admin
      );
    }
  }
);

test(
  specTitle(["SPC-048"], "bug: NEWFRONT-465 anonymous reactions lose the board view on the way to sign-in"),
  { tag: specTags(["SPC-048"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[2]!;
    const admin = await signInSession(seed.email, seed.password);
    const guestCreds = requireGuest(seed);
    const guest = await signInSession(guestCreds.email, guestCreds.password);
    const before = await setBoardFlags(
      seed.workspaceSlug,
      seed.projectId,
      { comments: false, reactions: true, votes: false },
      admin
    );
    const partyCode = "127881";
    const partyEmoji = "🎉";
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      await deleteReactionIfPresent(anchor, issueId, partyCode, admin);
      await deleteReactionIfPresent(anchor, issueId, partyCode, guest);
      await serverSpaceReact(anchor, issueId, partyCode, guest);
      const reactionsBefore = await serverSpaceIssueEngagement(anchor, issueId);

      await test.step("open the issue signed out from the kanban board", async () => {
        await driver.spaceCommentOpenIssue(anchor, issueName, "kanban");
        expect(await driver.spaceReactGroups()).toHaveLength(1);
      });

      await test.step("reacting lands on the auth gate keeping the issue but dropping the view", async () => {
        await driver.spaceReactClickGroup(partyEmoji);
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
        expect(await serverSpaceIssueEngagement(anchor, issueId)).toEqual(reactionsBefore);
      });

      await test.step("clean up the guest reaction", async () => {
        await deleteReactionIfPresent(anchor, issueId, partyCode, guest);
      });
    } finally {
      await setBoardFlags(
        seed.workspaceSlug,
        seed.projectId,
        { comments: before.comments, reactions: before.reactions, votes: before.votes },
        admin
      );
    }
  }
);

test(
  specTitle(["SPC-048"], "embedded boards offer no reaction add control"),
  { tag: specTags(["SPC-048"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueName = seed.issueNames[2]!;
    const admin = await signInSession(seed.email, seed.password);
    const guestCreds = requireGuest(seed);
    const guest = await signInSession(guestCreds.email, guestCreds.password);
    const before = await setBoardFlags(
      seed.workspaceSlug,
      seed.projectId,
      { comments: false, reactions: true, votes: false },
      admin
    );
    const partyCode = "127881";
    try {
      const issueId = await serverSpaceIssueIdByName(anchor, issueName);
      await deleteReactionIfPresent(anchor, issueId, partyCode, admin);
      await deleteReactionIfPresent(anchor, issueId, partyCode, guest);
      await serverSpaceReact(anchor, issueId, partyCode, guest);

      await test.step("open the issue inside an embedded frame while signed in", async () => {
        await driver.page.context().addCookies(sessionBrowserCookies(admin));
        await driver.spaceCommentOpenEmbed(anchor, issueName);
      });

      await test.step("the embedded overlay offers no reaction add control", async () => {
        expect(await driver.spaceReactEmbedAddVisible()).toBe(false);
      });

      await test.step("clean up the guest reaction", async () => {
        await deleteReactionIfPresent(anchor, issueId, partyCode, guest);
      });
    } finally {
      await setBoardFlags(
        seed.workspaceSlug,
        seed.projectId,
        { comments: before.comments, reactions: before.reactions, votes: before.votes },
        admin
      );
    }
  }
);
