// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-186): a workspace admin edits the workspace
// default of an overridable section; the editor seeds from the baseline and
// Save arms only on a dirty draft. BUG (NEWFRONT-193): the UI save 403s on
// CSRF — the prompting views enforce CSRF while the rest of /api does not —
// so the scenario locks in the inline CSRF failure and proves the intended
// flow through the CSRF-paired API plus the card re-read.
// Row: AGT-024.
import { test, expect } from "../../fixtures";
import { serverPromptSections, serverPromptSectionUpsert, serverPromptSectionUpsertStatus } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatMember } from "../support";
import { EDITABLE_SECTION, LOCKED_SECTION, expectCard } from "./support";

const ROWS = ["AGT-024"];
const WORKSPACE_BODY = "Parity AGT-024 workspace default.\n\nApplies to every member.";

test(
  specTitle(ROWS, "bug: NEWFRONT-193 workspace save 403s CSRF; seeding, gating and the intended API flow hold"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt24"));
    const { owner, ownerSession, workspaceSlug } = harness;

    const baseline = await test.step("server reports the pristine baseline", async () => {
      const rows = await serverPromptSections(workspaceSlug, "coding-task", "workspace", ownerSession);
      const found = rows.find((row) => row.key === EDITABLE_SECTION);
      expect(found?.source).toBe("default");
      if (found === undefined) throw new Error("[parity] expected the autonomy baseline.");
      return found;
    });

    await test.step("owner signs in and opens the prompts page", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
    });

    await test.step("workspace editing shows only for admins on permitted sections", async () => {
      const card = await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body !== "");
      expect(card.workspaceEditLabel).toBe("Customize for workspace");
      const locked = await expectCard(driver, LOCKED_SECTION, (candidate) => candidate.body !== "");
      expect(locked.workspaceEditLabel).toBeNull();
    });

    await test.step("the editor seeds from the baseline with Save disarmed", async () => {
      await driver.promptsOpenSectionEditor(EDITABLE_SECTION, "workspace");
      const state = await driver.promptsEditorState();
      expect(state?.scopeLabel).toContain("Workspace default");
      expect(state?.draft).toBe(baseline.body);
      expect(state?.saveEnabled).toBe(false);
      expect(state?.revertVisible).toBe(false);
      expect(state?.error).toBeNull();
    });

    await test.step("bug: the UI save fails inline on CSRF and stores nothing", async () => {
      await driver.promptsEditorFill(WORKSPACE_BODY);
      expect((await driver.promptsEditorState())?.saveEnabled).toBe(true);
      await driver.promptsEditorSave();
      await expect.poll(async () => (await driver.promptsEditorState())?.error ?? "", { timeout: 30_000 }).not.toBe("");
      const state = await driver.promptsEditorState();
      expect(state?.error).toContain("CSRF Failed");
      expect(state?.draft).toBe(WORKSPACE_BODY);
      const rows = await serverPromptSections(workspaceSlug, "coding-task", "workspace", ownerSession);
      expect(rows.find((row) => row.key === EDITABLE_SECTION)?.source).toBe("default");
      await driver.promptsEditorCancel();
    });

    await test.step("intended flow: the API stores the default and the card refreshes to it", async () => {
      await serverPromptSectionUpsert(workspaceSlug, EDITABLE_SECTION, "workspace", WORKSPACE_BODY, ownerSession);
      await driver.promptsOpen(workspaceSlug);
      const card = await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body === WORKSPACE_BODY);
      expect(card.sourceBadge).toBe("Workspace override");
      expect(card.workspaceEditLabel).toBe("Edit workspace default");
      await driver.promptsOpenSectionEditor(EDITABLE_SECTION, "workspace");
      const state = await driver.promptsEditorState();
      expect(state?.draft).toBe(WORKSPACE_BODY);
      expect(state?.saveEnabled).toBe(false);
      expect(state?.revertVisible).toBe(true);
      await driver.promptsEditorCancel();
    });

    await test.step("a failing save surfaces inline and keeps the editor open", async () => {
      await driver.promptsOpenSectionEditor(EDITABLE_SECTION, "workspace");
      await driver.promptsEditorFill(`${WORKSPACE_BODY}\n\nNever persisted.`);
      await driver.promptsFailUpsertOnce();
      await driver.promptsEditorSave();
      await expect.poll(async () => (await driver.promptsEditorState())?.error ?? "", { timeout: 30_000 }).not.toBe("");
      const state = await driver.promptsEditorState();
      expect(state?.error).toContain("parity upsert failure");
      expect(state?.draft).toContain("Never persisted.");
      const rows = await serverPromptSections(workspaceSlug, "coding-task", "workspace", ownerSession);
      expect(rows.find((row) => row.key === EDITABLE_SECTION)?.body).toBe(WORKSPACE_BODY);
      await driver.promptsEditorCancel();
    });

    await test.step("members see no workspace editing and the API refuses their writes", async () => {
      const member = await seatMember(harness);
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
      const card = await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body !== "");
      expect(card.workspaceEditLabel).toBeNull();
      const refused = await serverPromptSectionUpsertStatus(
        workspaceSlug,
        EDITABLE_SECTION,
        "workspace",
        "Member write attempt.",
        member.session
      );
      expect(refused.status).toBe(403);
      expect(refused.payload).toEqual({ error: "forbidden" });
    });
  }
);
