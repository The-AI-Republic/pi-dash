// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-186): the receipt tab shows one card per agent
// kind with its ordered composed sections and a collapsible full assembled
// template matching the compiled API read; the side nav lists the kinds
// with counts and anchors. bug: NEWFRONT-194 — the old app's count badge
// and nav counts render the literal `{{count}} sections` template (an
// i18next-style placeholder under the ICU `t()`); the intended target, and
// what apps/web_new ships, renders the resolved section count. The gate
// asserts both targets so the row proves the target, not the old bug.
// Row: AGT-028.
import { test, expect } from "../../fixtures";
import { serverPromptCompiled, serverPromptSections } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatMember } from "../support";
import { PROMPT_KINDS } from "./support";

const ROWS = ["AGT-028"];

/** The count copy the intended target renders for `count` sections. */
function countLabel(count: number): string {
  return count === 1 ? "1 section" : `${count} sections`;
}

test(
  specTitle(ROWS, "bug: NEWFRONT-194 receipts show the literal count template; numbering and assembly hold"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt28"));
    const { owner, ownerSession, workspaceSlug } = harness;

    const serverSections = await test.step("server resolves sections and receipts", async () => {
      const sections = new Map<string, { key: string; title: string; source: string }[]>();
      const compiled = new Map<string, string>();
      for (const kind of PROMPT_KINDS) {
        const rows = await serverPromptSections(workspaceSlug, kind, "user", ownerSession);
        sections.set(
          kind,
          rows.map((row) => ({ key: row.key, title: row.title, source: row.source }))
        );
        compiled.set(kind, (await serverPromptCompiled(workspaceSlug, kind, "user", ownerSession)).template_body);
      }
      return { sections, compiled };
    });

    await test.step("owner signs in and opens the receipt tab", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
      await driver.promptsOpenTab("Receipt");
      expect(await driver.promptsActiveTab()).toBe("Receipt");
    });

    await test.step("each card numbers its sections with a rendered count badge", async () => {
      let cards = await driver.promptsReceiptCards();
      await expect.poll(async () => (await driver.promptsReceiptCards()).length, { timeout: 30_000 }).toBe(3);
      cards = await driver.promptsReceiptCards();
      expect(cards.map((card) => card.kind).sort()).toEqual([...PROMPT_KINDS].sort());
      for (const card of cards) {
        const expected = serverSections.sections.get(card.kind) ?? [];
        expect(expected.length).toBeGreaterThan(0);
        // bug: NEWFRONT-194 — the old app renders the raw template; the
        // intended target renders the resolved count (apps/web_new).
        expect(card.countBadge).toBe(driver.target === "web_new" ? countLabel(expected.length) : "{{count}} sections");
        expect(card.sections.map((section) => section.key)).toEqual(expected.map((row) => row.key));
        card.sections.forEach((section, index) => {
          expect(section.num).toBe(String(index + 1).padStart(2, "0"));
          expect(section.title).toBe(expected[index]?.title ?? "");
          expect(section.sourceBadge).toBe("Pi Dash default");
        });
        expect(await driver.promptsReceiptExpanded(card.kind)).toBe(false);
        expect(await driver.promptsReceiptTemplate(card.kind)).toBeNull();
      }
    });

    await test.step("expanding shows the assembled template and collapses again", async () => {
      for (const kind of PROMPT_KINDS) {
        await driver.promptsReceiptToggle(kind);
        expect(await driver.promptsReceiptExpanded(kind)).toBe(true);
        const template = await driver.promptsReceiptTemplate(kind);
        expect(template).toBe((serverSections.compiled.get(kind) ?? "").trimEnd());
        await driver.promptsReceiptToggle(kind);
        expect(await driver.promptsReceiptExpanded(kind)).toBe(false);
        expect(await driver.promptsReceiptTemplate(kind)).toBeNull();
      }
    });

    await test.step("side nav lists kinds with counts and anchors", async () => {
      const nav = await driver.promptsReceiptNav();
      expect(nav.map((entry) => entry.kind).sort()).toEqual([...PROMPT_KINDS].sort());
      for (const entry of nav) {
        const expected = serverSections.sections.get(entry.kind) ?? [];
        expect(expected.length).toBeGreaterThan(0);
        // Same deviation as the badge: raw template on the old app, the
        // resolved count on the intended target.
        expect(entry.count).toBe(driver.target === "web_new" ? countLabel(expected.length) : "{{count}} sections");
      }
      const hash = await driver.promptsReceiptNavJump("review");
      expect(hash).toBe("#prompt-receipt-review");
    });

    await test.step("member reads the same receipts", async () => {
      const member = await seatMember(harness);
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
      await driver.promptsOpenTab("Receipt");
      await expect.poll(async () => (await driver.promptsReceiptCards()).length, { timeout: 30_000 }).toBe(3);
      const cards = await driver.promptsReceiptCards();
      for (const card of cards) {
        const expected = serverSections.sections.get(card.kind) ?? [];
        expect(card.countBadge).toBe(driver.target === "web_new" ? countLabel(expected.length) : "{{count}} sections");
      }
      await driver.promptsReceiptToggle("coding-task");
      expect(await driver.promptsReceiptTemplate("coding-task")).toBe(
        (serverSections.compiled.get("coding-task") ?? "").trimEnd()
      );
    });
  }
);
