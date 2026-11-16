// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-186): the prompts page shows a loading line
// until the section lists resolve, a member-list failure raises an error
// banner instead of a blank page, and a workspace-baseline failure warns
// the admin while hiding (not silently dropping) the workspace editing
// controls. Members never fetch the baseline, so they never see its
// warning.
// Row: AGT-032.
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatMember } from "../support";
import { EDITABLE_SECTION, expectCard } from "./support";

const ROWS = ["AGT-032"];

test(
  specTitle(ROWS, "loading line, member-list banner and admin-only baseline warning guide instead of blanking"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace and member", async () =>
      schedulerHarness("parity-agt32"));
    const { owner, workspaceSlug } = harness;
    const member = await seatMember(harness);

    await test.step("owner signs in", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
    });

    await test.step("a loading line shows until the lists resolve", async () => {
      await driver.promptsDelaySectionsOnce(3000);
      await driver.promptsOpen(workspaceSlug);
      expect(await driver.promptsLoadingVisible()).toBe(true);
      await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body !== "");
      expect(await driver.promptsLoadingVisible()).toBe(false);
      expect(await driver.promptsSectionsErrorVisible()).toBe(false);
    });

    await test.step("member-list failure raises an error banner", async () => {
      await driver.promptsFailSectionsStart("user");
      try {
        await driver.promptsOpen(workspaceSlug);
        await expect.poll(async () => driver.promptsSectionsErrorVisible(), { timeout: 30_000 }).toBe(true);
        expect(await driver.promptsSectionCards()).toEqual([]);
      } finally {
        await driver.promptsFailSectionsStop();
      }
      await driver.promptsOpen(workspaceSlug);
      await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body !== "");
      expect(await driver.promptsSectionsErrorVisible()).toBe(false);
    });

    await test.step("baseline failure warns the admin and hides workspace editing", async () => {
      await driver.promptsFailSectionsStart("workspace");
      try {
        await driver.promptsOpen(workspaceSlug);
        await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body !== "");
        await expect.poll(async () => driver.promptsWorkspaceWarningVisible(), { timeout: 30_000 }).toBe(true);
        // The member lists still render, but no workspace editing is
        // offered anywhere while the baseline is unknown.
        const cards = await driver.promptsSectionCards();
        expect(cards.length).toBeGreaterThan(0);
        for (const card of cards) {
          expect(card.workspaceEditLabel).toBeNull();
        }
        const editable = await driver.promptsSectionCard(EDITABLE_SECTION);
        expect(editable?.personalEditLabel).not.toBeNull();
      } finally {
        await driver.promptsFailSectionsStop();
      }
      await driver.promptsOpen(workspaceSlug);
      const recovered = await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body !== "");
      expect(recovered.workspaceEditLabel).not.toBeNull();
      expect(await driver.promptsWorkspaceWarningVisible()).toBe(false);
    });

    await test.step("members never see the baseline warning", async () => {
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.promptsFailSectionsStart("workspace");
      try {
        await driver.promptsOpen(workspaceSlug);
        await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body !== "");
        expect(await driver.promptsWorkspaceWarningVisible()).toBe(false);
        expect(await driver.promptsSectionsErrorVisible()).toBe(false);
      } finally {
        await driver.promptsFailSectionsStop();
      }
    });
  }
);
