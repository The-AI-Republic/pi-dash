// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-186): reverting an override asks for explicit
// confirmation with scope-specific copy and an irreversibility warning, and
// cancel keeps the override. BUG (NEWFRONT-193): the UI confirm 403s on
// CSRF — the dialog closes and the failure lands inline with the override
// kept — so the scenario locks that in and proves the intended revert
// through the CSRF-paired API plus the card re-read.
// Row: AGT-027.
import { test, expect } from "../../fixtures";
import {
  serverPromptSections,
  serverPromptSectionRevert,
  serverPromptSectionRevertStatus,
  serverPromptSectionUpsert,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatMember } from "../support";
import { EDITABLE_SECTION, expectCard } from "./support";

const ROWS = ["AGT-027"];
const PERSONAL_BODY = "Parity AGT-027 personal override.";
const WORKSPACE_BODY = "Parity AGT-027 workspace default.";

test(
  specTitle(ROWS, "bug: NEWFRONT-193 revert confirm 403s CSRF; dialog copy, cancel and the intended API revert hold"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace and member", async () =>
      schedulerHarness("parity-agt27"));
    const { owner, ownerSession, workspaceSlug } = harness;
    const member = await seatMember(harness);

    await test.step("both scopes hold an override", async () => {
      await serverPromptSectionUpsert(workspaceSlug, EDITABLE_SECTION, "workspace", WORKSPACE_BODY, ownerSession);
      await serverPromptSectionUpsert(workspaceSlug, EDITABLE_SECTION, "user", PERSONAL_BODY, member.session);
    });

    await test.step("member signs in and opens the personal editor", async () => {
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
      await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body === PERSONAL_BODY);
      await driver.promptsOpenSectionEditor(EDITABLE_SECTION, "user");
      expect((await driver.promptsEditorState())?.revertVisible).toBe(true);
    });

    await test.step("personal revert explains itself with scope copy", async () => {
      await driver.promptsEditorRevertOpen();
      const dialog = await driver.promptsRevertDialog();
      expect(dialog?.title).toBe("Revert to default?");
      expect(dialog?.body).toContain("you trigger");
      expect(dialog?.body).toContain("can't be undone");
      expect(dialog?.confirmLabel).toBe("Revert");
      await driver.promptsRevertCancel();
      expect(await driver.promptsRevertDialog()).toBeNull();
      expect((await driver.promptsEditorState())?.draft).toBe(PERSONAL_BODY);
    });

    await test.step("bug: the UI confirm fails on CSRF with the override kept", async () => {
      await driver.promptsEditorRevertOpen();
      await driver.promptsRevertConfirm();
      // The dialog closes and the failure lands inline behind it.
      await expect.poll(async () => (await driver.promptsEditorState())?.error ?? "", { timeout: 30_000 }).not.toBe("");
      await expect.poll(async () => driver.promptsRevertDialog(), { timeout: 30_000 }).toBeNull();
      const state = await driver.promptsEditorState();
      // The revert path reports only its generic message (it drops the
      // server's detail, unlike the save path), while the override stays.
      expect(state?.error).toBe("Could not revert the section.");
      const rows = await serverPromptSections(workspaceSlug, "coding-task", "user", member.session);
      expect(rows.find((row) => row.key === EDITABLE_SECTION)?.body).toBe(PERSONAL_BODY);
      await driver.promptsEditorCancel();
    });

    await test.step("intended flow: the API revert drops the personal override", async () => {
      await serverPromptSectionRevert(workspaceSlug, EDITABLE_SECTION, "user", member.session);
      await driver.promptsOpen(workspaceSlug);
      // The card falls back to the workspace row.
      const card = await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body === WORKSPACE_BODY);
      expect(card.sourceBadge).toBe("Workspace override");
    });

    await test.step("workspace revert cancel keeps the override", async () => {
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
      await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body === WORKSPACE_BODY);
      await driver.promptsOpenSectionEditor(EDITABLE_SECTION, "workspace");
      await driver.promptsEditorRevertOpen();
      const dialog = await driver.promptsRevertDialog();
      expect(dialog?.body).toContain("every member");
      expect(dialog?.body).toContain("can't be undone");
      await driver.promptsRevertCancel();
      expect(await driver.promptsRevertDialog()).toBeNull();
      expect((await driver.promptsEditorState())?.draft).toBe(WORKSPACE_BODY);
      const rows = await serverPromptSections(workspaceSlug, "coding-task", "workspace", ownerSession);
      expect(rows.find((row) => row.key === EDITABLE_SECTION)?.body).toBe(WORKSPACE_BODY);
      await driver.promptsEditorCancel();
    });

    await test.step("intended flow: the API revert restores the registry default", async () => {
      await serverPromptSectionRevert(workspaceSlug, EDITABLE_SECTION, "workspace", ownerSession);
      await driver.promptsOpen(workspaceSlug);
      const card = await expectCard(
        driver,
        EDITABLE_SECTION,
        (candidate) => candidate.sourceBadge === "Pi Dash default"
      );
      expect(card.workspaceEditLabel).toBe("Customize for workspace");
      const gone = await serverPromptSectionRevertStatus(workspaceSlug, EDITABLE_SECTION, "workspace", ownerSession);
      expect(gone.status).toBe(404);
      const missing = await serverPromptSectionRevertStatus(workspaceSlug, EDITABLE_SECTION, "user", member.session);
      expect(missing.status).toBe(404);
    });
  }
);
