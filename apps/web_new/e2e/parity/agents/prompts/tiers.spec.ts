// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-186): locked sections refuse edits at every
// tier — no editing controls for anyone in the UI, and the API refuses
// both scopes with a locked message. Gap: the live registry ships no
// `workspace`-tier (admin-managed) section, so that half is unprovable;
// the scenario asserts the absence instead of the behavior.
// Row: AGT-033.
import { test, expect } from "../../fixtures";
import { serverPromptSections, serverPromptSectionUpsertStatus } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatMember } from "../support";
import { PROMPT_KINDS, expectCard } from "./support";

const ROWS = ["AGT-033"];

test(
  specTitle(ROWS, "locked sections refuse edits at every tier; no admin-managed tier ships"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace and member", async () =>
      schedulerHarness("parity-agt33"));
    const { owner, ownerSession, workspaceSlug } = harness;
    const member = await seatMember(harness);

    const tiers = await test.step("server reports the live tiers", async () => {
      const byKey = new Map<string, string>();
      for (const kind of PROMPT_KINDS) {
        for (const row of await serverPromptSections(workspaceSlug, kind, "user", ownerSession)) {
          byKey.set(row.key, row.customizable);
        }
      }
      const locked = [...byKey.entries()].filter(([, tier]) => tier === "locked").map(([key]) => key);
      const managed = [...byKey.entries()].filter(([, tier]) => tier === "workspace").map(([key]) => key);
      expect(locked.length).toBeGreaterThan(0);
      return { locked, managed };
    });

    await test.step("owner signs in and opens the prompts page", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
    });

    await test.step("locked sections offer no editing to the admin and carry the mark", async () => {
      for (const key of tiers.locked) {
        const card = await expectCard(driver, key, (candidate) => candidate.body !== "");
        expect(card.badges).toContain("Locked");
        expect(card.workspaceEditLabel).toBeNull();
        expect(card.personalEditLabel).toBeNull();
      }
    });

    await test.step("locked sections offer no editing to members either", async () => {
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
      const sample = tiers.locked.slice(0, 3);
      expect(sample.length).toBeGreaterThan(0);
      for (const key of sample) {
        const card = await expectCard(driver, key, (candidate) => candidate.body !== "");
        expect(card.badges).toContain("Locked");
        expect(card.workspaceEditLabel).toBeNull();
        expect(card.personalEditLabel).toBeNull();
      }
    });

    await test.step("the API refuses locked writes at both scopes", async () => {
      const key = tiers.locked[0] ?? "";
      if (key === "") throw new Error("[parity] expected a locked section.");
      for (const scope of ["workspace", "user"] as const) {
        const refused = await serverPromptSectionUpsertStatus(
          workspaceSlug,
          key,
          scope,
          "Locked write attempt.",
          scope === "workspace" ? ownerSession : member.session
        );
        expect(refused.status).toBe(403);
        expect(String((refused.payload as { error?: unknown }).error ?? "")).toContain("locked");
      }
    });

    await test.step("no admin-managed section ships in the live registry", async () => {
      expect(tiers.managed).toEqual([]);
      const cards = await driver.promptsSectionCards();
      for (const card of cards) {
        expect(card.badges).not.toContain("Admin-managed");
      }
    });
  }
);
