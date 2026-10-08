// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-186): a workspace member keeps a personal
// override of an overridable section; the editor seeds from the effective
// body and locked sections offer no personal editing. BUG (NEWFRONT-193):
// the UI save 403s on CSRF, so the scenario locks in the inline CSRF
// failure and proves the intended flow through the CSRF-paired API plus
// the card re-read, including that nobody else's view changes.
// Row: AGT-025.
import { test, expect } from "../../fixtures";
import { serverPromptSections, serverPromptSectionUpsert, serverPromptSectionUpsertStatus } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatMember } from "../support";
import { EDITABLE_SECTION, LOCKED_SECTION, expectCard } from "./support";

const ROWS = ["AGT-025"];
const PERSONAL_BODY = "Parity AGT-025 personal override.\n\nOnly my runs use this.";

test(
  specTitle(ROWS, "bug: NEWFRONT-193 personal save 403s CSRF; seeding, gating and the intended API flow hold"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace and member", async () =>
      schedulerHarness("parity-agt25"));
    const { owner, ownerSession, workspaceSlug } = harness;
    const member = await seatMember(harness);

    await test.step("member signs in and opens the prompts page", async () => {
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
    });

    await test.step("personal editing shows only on permitted sections", async () => {
      const card = await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body !== "");
      expect(card.personalEditLabel).toBe("Customize for me");
      const locked = await expectCard(driver, LOCKED_SECTION, (candidate) => candidate.body !== "");
      expect(locked.personalEditLabel).toBeNull();
    });

    await test.step("the editor seeds from the effective body with Save disarmed", async () => {
      const effective = (await serverPromptSections(workspaceSlug, "coding-task", "user", member.session)).find(
        (row) => row.key === EDITABLE_SECTION
      );
      await driver.promptsOpenSectionEditor(EDITABLE_SECTION, "user");
      const state = await driver.promptsEditorState();
      expect(state?.scopeLabel).toContain("personal override");
      expect(state?.draft).toBe(effective?.body ?? "");
      expect(state?.saveEnabled).toBe(false);
      expect(state?.revertVisible).toBe(false);
    });

    await test.step("bug: the UI save fails inline on CSRF and stores nothing", async () => {
      await driver.promptsEditorFill(PERSONAL_BODY);
      expect((await driver.promptsEditorState())?.saveEnabled).toBe(true);
      await driver.promptsEditorSave();
      await expect.poll(async () => (await driver.promptsEditorState())?.error ?? "", { timeout: 30_000 }).not.toBe("");
      const state = await driver.promptsEditorState();
      expect(state?.error).toContain("CSRF Failed");
      expect(state?.draft).toBe(PERSONAL_BODY);
      const rows = await serverPromptSections(workspaceSlug, "coding-task", "user", member.session);
      expect(rows.find((row) => row.key === EDITABLE_SECTION)?.source).toBe("default");
      await driver.promptsEditorCancel();
    });

    await test.step("intended flow: the API stores the override and the card marks it personal", async () => {
      await serverPromptSectionUpsert(workspaceSlug, EDITABLE_SECTION, "user", PERSONAL_BODY, member.session);
      await driver.promptsOpen(workspaceSlug);
      const card = await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body === PERSONAL_BODY);
      expect(card.sourceBadge).toBe("Your override");
      expect(card.personalEditLabel).toBe("Edit my override");
    });

    await test.step("nobody else's view changes", async () => {
      const rows = await serverPromptSections(workspaceSlug, "coding-task", "user", ownerSession);
      expect(rows.find((row) => row.key === EDITABLE_SECTION)?.source).toBe("default");
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
      const card = await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body !== "");
      expect(card.sourceBadge).toBe("Pi Dash default");
      expect(card.personalEditLabel).toBe("Customize for me");
    });

    await test.step("a failing save surfaces inline and keeps the editor open", async () => {
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
      await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body === PERSONAL_BODY);
      await driver.promptsOpenSectionEditor(EDITABLE_SECTION, "user");
      await driver.promptsEditorFill(`${PERSONAL_BODY}\n\nNever persisted.`);
      await driver.promptsFailUpsertOnce();
      await driver.promptsEditorSave();
      await expect.poll(async () => (await driver.promptsEditorState())?.error ?? "", { timeout: 30_000 }).not.toBe("");
      const state = await driver.promptsEditorState();
      expect(state?.error).toContain("parity upsert failure");
      expect(state?.draft).toContain("Never persisted.");
      const rows = await serverPromptSections(workspaceSlug, "coding-task", "user", member.session);
      expect(rows.find((row) => row.key === EDITABLE_SECTION)?.body).toBe(PERSONAL_BODY);
      await driver.promptsEditorCancel();
    });

    await test.step("the API refuses personal writes on locked sections", async () => {
      const refused = await serverPromptSectionUpsertStatus(
        workspaceSlug,
        LOCKED_SECTION,
        "user",
        "Locked write attempt.",
        member.session
      );
      expect(refused.status).toBe(403);
      expect(String((refused.payload as { error?: unknown }).error ?? "")).toContain("locked");
    });
  }
);
