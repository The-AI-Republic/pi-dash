// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-186): the section editor compares the draft
// against the pristine default side by side, cancel discards the draft, and
// the revert affordance appears only when an override exists at the edited
// scope (a workspace override never arms the personal editor's revert).
// Row: AGT-026.
import { test, expect } from "../../fixtures";
import { serverPromptSections, serverPromptSectionUpsert } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { EDITABLE_SECTION, expectCard } from "./support";

const ROWS = ["AGT-026"];
const WORKSPACE_BODY = "Parity AGT-026 workspace default.";
const DRAFT_BODY = "Parity AGT-026 unsaved draft.";

test(
  specTitle(ROWS, "editor compares against the pristine default; cancel discards; revert arms per scope"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt26"));
    const { owner, ownerSession, workspaceSlug } = harness;

    const baseline = await test.step("server reports the pristine baseline", async () => {
      const rows = await serverPromptSections(workspaceSlug, "coding-task", "workspace", ownerSession);
      const found = rows.find((row) => row.key === EDITABLE_SECTION);
      if (found === undefined) throw new Error("[parity] expected the autonomy baseline.");
      return found.default_body.trimEnd();
    });

    await test.step("owner signs in and opens the prompts page", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
      await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body !== "");
    });

    await test.step("compare shows the draft beside the pristine default", async () => {
      await driver.promptsOpenSectionEditor(EDITABLE_SECTION, "user");
      let state = await driver.promptsEditorState();
      expect(state?.defaultVisible).toBe(false);
      expect(state?.defaultBody).toBeNull();
      await driver.promptsEditorFill(DRAFT_BODY);
      await driver.promptsEditorToggleCompare();
      state = await driver.promptsEditorState();
      expect(state?.defaultVisible).toBe(true);
      expect(state?.defaultBody).toBe(baseline);
      expect(state?.draft).toBe(DRAFT_BODY);
      // Toggling again hides the pane without touching the draft.
      await driver.promptsEditorToggleCompare();
      state = await driver.promptsEditorState();
      expect(state?.defaultVisible).toBe(false);
      expect(state?.draft).toBe(DRAFT_BODY);
    });

    await test.step("cancel discards the draft", async () => {
      await driver.promptsEditorCancel();
      expect(await driver.promptsEditorState()).toBeNull();
      const card = await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body !== "");
      expect(card.body).toBe(baseline);
      expect(card.sourceBadge).toBe("Pi Dash default");
      const rows = await serverPromptSections(workspaceSlug, "coding-task", "user", ownerSession);
      expect(rows.find((row) => row.key === EDITABLE_SECTION)?.source).toBe("default");
    });

    await test.step("revert arms only where an override exists at the edited scope", async () => {
      await serverPromptSectionUpsert(workspaceSlug, EDITABLE_SECTION, "workspace", WORKSPACE_BODY, ownerSession);
      await driver.promptsOpen(workspaceSlug);
      // Workspace editor: the override exists here, so revert shows.
      await driver.promptsOpenSectionEditor(EDITABLE_SECTION, "workspace");
      expect((await driver.promptsEditorState())?.revertVisible).toBe(true);
      await driver.promptsEditorCancel();
      // Personal editor: no personal override exists, so no revert —
      // even though the effective body now comes from the workspace row.
      await driver.promptsOpenSectionEditor(EDITABLE_SECTION, "user");
      const state = await driver.promptsEditorState();
      expect(state?.revertVisible).toBe(false);
      expect(state?.draft).toBe(WORKSPACE_BODY);
      await driver.promptsEditorCancel();
    });
  }
);
