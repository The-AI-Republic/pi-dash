// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-186): the prompts sections browser shows one
// card per section with its effective body, provenance badge, governance
// and kind marks, and per-scope edit affordances; the side nav jumps to
// each card anchor; members and guests read the same list while the
// workspace baseline stays an admin-only read in the UI.
// Row: AGT-023.
import { test, expect } from "../../fixtures";
import { serverPromptSections, type ParityPromptSection } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatGuest, seatMember } from "../support";

const ROWS = ["AGT-023"];
const KINDS = ["coding-task", "review", "scheduler"] as const;
const KIND_LABELS: Record<string, string> = {
  "coding-task": "Coding task",
  review: "Review",
  scheduler: "Scheduler",
};

/** Poll the sections browser until its card keys match (lists resolve async). */
async function expectCardKeys(
  driver: { promptsSectionCards: () => Promise<{ key: string }[]> },
  keys: string[]
): Promise<void> {
  const wanted = [...keys].sort();
  await expect
    .poll(async () => (await driver.promptsSectionCards()).map((card) => card.key).sort(), { timeout: 30_000 })
    .toEqual(wanted);
}

test(
  specTitle(ROWS, "sections browser shows effective bodies with provenance; nav jumps; members and guests read along"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt23"));
    const { owner, ownerSession, workspaceSlug } = harness;

    const serverByKind = await test.step("server resolves three kinds for the owner", async () => {
      const out = new Map<string, ParityPromptSection[]>();
      for (const kind of KINDS) {
        out.set(kind, await serverPromptSections(workspaceSlug, kind, "user", ownerSession));
      }
      expect(out.get("coding-task")?.length).toBeGreaterThan(0);
      expect(out.get("review")?.length).toBeGreaterThan(0);
      expect(out.get("scheduler")?.length).toBeGreaterThan(0);
      return out;
    });
    const unionKeys = [...new Set([...serverByKind.values()].flat().map((section) => section.key))];
    const kindsOf = (key: string): string[] =>
      KINDS.filter((kind) => (serverByKind.get(kind) ?? []).some((section) => section.key === key));
    const effectiveOf = (key: string): ParityPromptSection => {
      const found = [...serverByKind.values()].flat().find((section) => section.key === key);
      if (found === undefined) throw new Error(`[parity] expected a server section for ${key}.`);
      return found;
    };

    await test.step("owner signs in and opens the prompts page", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
      expect(await driver.promptsActiveTab()).toBe("Sections");
    });

    await test.step("every section renders a card with provenance, marks and its effective body", async () => {
      await expectCardKeys(driver, unionKeys);
      const cards = await driver.promptsSectionCards();
      expect(cards.length).toBe(unionKeys.length);
      for (const card of cards) {
        const server = effectiveOf(card.key);
        expect(card.title).toBe(server.title);
        // Fresh workspace: no overrides anywhere, so every card resolves
        // to the registry default.
        expect(card.sourceBadge).toBe("Pi Dash default");
        expect(card.kinds).toEqual(kindsOf(card.key).map((kind) => KIND_LABELS[kind]));
        expect(card.staleWarning).toBe(false);
        expect(card.body).toBe(server.body.trimEnd());
        if (server.customizable === "locked") {
          expect(card.badges).toContain("Locked");
          expect(card.workspaceEditLabel).toBeNull();
          expect(card.personalEditLabel).toBeNull();
        } else {
          expect(card.badges).not.toContain("Locked");
          expect(card.workspaceEditLabel).not.toBeNull();
          expect(card.personalEditLabel).not.toBeNull();
        }
        expect(card.badges).not.toContain("Admin-managed");
      }
    });

    await test.step("side nav lists every card and jumps to its anchor", async () => {
      const nav = await driver.promptsSectionNav();
      expect(nav.map((entry) => entry.key).sort()).toEqual([...unionKeys].sort());
      for (const entry of nav) {
        expect(entry.title).toBe(effectiveOf(entry.key).title);
      }
      const target = unionKeys[Math.floor(unionKeys.length / 2)] ?? unionKeys[0];
      if (target === undefined) throw new Error("[parity] expected at least one section.");
      const hash = await driver.promptsSectionNavJump(target);
      expect(hash).toBe(`#prompt-section-${target}`);
      expect(await driver.promptsSectionCard(target)).not.toBeNull();
    });

    await test.step("member reads the same list with personal-only editing", async () => {
      const member = await seatMember(harness);
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
      await expectCardKeys(driver, unionKeys);
      const cards = await driver.promptsSectionCards();
      expect(cards.length).toBe(unionKeys.length);
      for (const card of cards) {
        const server = effectiveOf(card.key);
        expect(card.body).toBe(server.body.trimEnd());
        // The workspace baseline read is admin-gated in the UI: members
        // never see workspace editing, only personal editing where allowed.
        expect(card.workspaceEditLabel).toBeNull();
        if (server.customizable === "overridable") {
          expect(typeof card.personalEditLabel).toBe("string");
        } else {
          expect(card.personalEditLabel).toBeNull();
        }
      }
      expect(await driver.promptsWorkspaceWarningVisible()).toBe(false);
    });

    await test.step("guest reads the same list", async () => {
      const guest = await seatGuest(harness);
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(guest.email, guest.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
      await expectCardKeys(driver, unionKeys);
      const cards = await driver.promptsSectionCards();
      expect(cards.length).toBe(unionKeys.length);
      expect(cards[0]?.body.length).toBeGreaterThan(0);
    });

    await test.step("the baseline split is UI-gated: the API serves workspace scope to members", async () => {
      const member = await seatMember(harness);
      const rows = await serverPromptSections(workspaceSlug, "coding-task", "workspace", member.session);
      expect(rows.length).toBeGreaterThan(0);
    });
  }
);
