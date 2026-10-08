// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-186): the receipt tab shows one card per agent
// kind with its ordered composed sections and a collapsible full assembled
// template matching the compiled API read; the side nav lists the kinds
// with counts and anchors. BUG (NEWFRONT-194): the count badge and nav
// counts render the literal `{{count}} sections` template (i18next-style
// placeholder under the ICU `t()`), so the scenario locks that in.
// Row: AGT-028.
import { test, expect } from "../../fixtures";
import { serverPromptCompiled, serverPromptSections } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatMember } from "../support";
import { PROMPT_KINDS } from "./support";

const ROWS = ["AGT-028"];

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
        // Bug: the badge renders the uninterpolated template.
        expect(card.countBadge).toBe("{{count}} sections");
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
        // Bug: the nav counts render the same uninterpolated template.
        expect(entry.count).toBe("{{count}} sections");
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
        expect(card.countBadge).toBe("{{count}} sections");
      }
      await driver.promptsReceiptToggle("coding-task");
      expect(await driver.promptsReceiptTemplate("coding-task")).toBe(
        (serverSections.compiled.get("coding-task") ?? "").trimEnd()
      );
    });
  }
);
