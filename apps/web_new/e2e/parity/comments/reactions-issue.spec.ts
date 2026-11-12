// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-116): react to a work item with an emoji — the
// picker adds a reaction, clicking the chip toggles it off, and the count
// plus who-reacted stay visible. Row: CMT-022.
import { test, expect } from "../fixtures";
import {
  serverCleanupIssueWithSession,
  serverCreateIssueFull,
  serverIssueReactions,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

// Cold dev servers compile route bundles on first load, which can take
// minutes on a loaded host; each step still carries its own poll timeout.
test.setTimeout(600_000);

test(
  specTitle(["CMT-022"], "react to a work item with toggle, count and reactor names"),
  { tag: specTags(["CMT-022"]) },
  async ({ driver, seed }) => {
    const tag = `NF116 issue ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      await test.step("sign in and open the fresh work item", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
        await driver.openIssueDetail(seed.workspaceSlug, seed.projectId, issue.id);
        // The trigger renders disabled while metadata hydrates; wait for
        // the enabled control before touching the picker.
        await expect.poll(() => driver.reactionsIssuePickerReady(), { timeout: 30_000 }).toBe(true);
        expect(await driver.issueReactionChips()).toEqual([]);
        expect(await serverIssueReactions(seed.workspaceSlug, seed.projectId, issue.id, session)).toEqual([]);
      });

      await test.step("picker add shows a highlighted chip with count and server state", async () => {
        const picked = await driver.issueAddReaction();
        await expect
          .poll(() => driver.issueReactionChips(), { timeout: 15_000 })
          .toEqual([{ emoji: picked.emoji, count: 1, reacted: true }]);
        // Notices first: success toasts dismiss within seconds, so the
        // server read below would outlive them.
        await expect.poll(() => driver.sawToast("Reaction created successfully"), { timeout: 15_000 }).toBe(true);
        const reactions = await serverIssueReactions(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(reactions).toHaveLength(1);
        expect(reactions[0]!.reaction).toBe(picked.code);
      });

      await test.step("hovering the chip names who reacted", async () => {
        const picked = (await driver.issueReactionChips())[0]!;
        const tooltip = await driver.reactionsIssueChipTooltipText(picked.emoji, "Parity Oracle");
        expect(tooltip).toContain("Parity Oracle");
      });

      await test.step("clicking the chip again removes the reaction", async () => {
        const picked = (await driver.issueReactionChips())[0]!;
        await driver.issueClickReactionChip(picked.emoji);
        await expect.poll(() => driver.issueReactionChips(), { timeout: 15_000 }).toEqual([]);
        await expect.poll(() => driver.sawToast("Reaction removed successfully"), { timeout: 15_000 }).toBe(true);
        await expect
          .poll(() => serverIssueReactions(seed.workspaceSlug, seed.projectId, issue.id, session), {
            timeout: 15_000,
          })
          .toEqual([]);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);
