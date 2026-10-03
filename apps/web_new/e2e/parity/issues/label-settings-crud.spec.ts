// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-122): label settings create/edit/delete —
// the inline name/color form validates inline, seeds a random palette
// color, rejects duplicate names with a toast, and persists through the
// issue-labels endpoints; renames land on every carrying issue; deletes
// confirm through a modal and toast on failure. Rows: ISS-226 (create
// a label inline), ISS-227 (edit / rename / recolor), ISS-228 (delete
// with confirmation).
//
// Observed behavior notes (inventory rows carry them at update time):
// the create form seeds one of ten palette colors at random; the empty
// and over-long names fail client-side ("Label title is required" /
// "…not exceed 255 characters") while the server 400s the same shapes;
// a duplicate name 400s {name:["LABEL_NAME_ALREADY_EXISTS"]} and toasts
// "Label already exists"; the issue attach field is label_ids (a
// {labels:[…]} PATCH 204-noops); deleting a label cascades it off
// issues; a failed update rolls the store back and toasts "Error while
// updating the label"; a failed delete leaves the modal open and toasts
// "Label could not be deleted. Please try again.".
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupLabel,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateLabel,
  serverCreateProjectWithFlags,
  serverIssue,
  serverLabelOrNull,
  serverLabels,
  serverSetIssueLabels,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

/** The ten preset palette colors as the browser reports them. */
const PALETTE_RGB = new Set([
  "rgb(255, 105, 0)",
  "rgb(252, 185, 0)",
  "rgb(123, 220, 181)",
  "rgb(0, 208, 132)",
  "rgb(142, 209, 252)",
  "rgb(6, 147, 227)",
  "rgb(171, 184, 195)",
  "rgb(235, 20, 76)",
  "rgb(247, 141, 167)",
  "rgb(153, 0, 239)",
]);

test(
  specTitle(["ISS-226"], "label create validates inline, picks a palette color, persists and reaches pickers"),
  { tag: specTags(["ISS-226"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 lblc ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    const name = `${tag} alpha`;
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);
      expect(await driver.settingsLabelsAddVisible()).toBe(true);

      await test.step("the create form seeds a random palette color", async () => {
        await driver.settingsLabelsOpenCreate();
        const dot = await driver.settingsLabelsDotColor();
        expect(PALETTE_RGB.has(dot)).toBe(true);
      });

      await test.step("empty and over-long names fail inline", async () => {
        await driver.settingsLabelsAttemptSubmit();
        await expect.poll(() => driver.settingsLabelsFormError(), { timeout: 10_000 }).toBe("Label title is required");
        await driver.settingsLabelsFillName("x".repeat(256));
        await driver.settingsLabelsAttemptSubmit();
        await expect
          .poll(() => driver.settingsLabelsFormError(), { timeout: 10_000 })
          .toBe("Label name should not exceed 255 characters");
      });

      await test.step("picking a swatch recolors the dot and persists on create", async () => {
        // The seed is random, so pick a color it demonstrably is not.
        const before = await driver.settingsLabelsDotColor();
        const pick = before === "rgb(0, 208, 132)" ? "#9900EF" : "#00D084";
        const pickRgb = before === "rgb(0, 208, 132)" ? "rgb(153, 0, 239)" : "rgb(0, 208, 132)";
        await driver.settingsLabelsOpenColorPicker();
        await driver.settingsLabelsPickColor(pick);
        await expect.poll(() => driver.settingsLabelsDotColor(), { timeout: 10_000 }).toBe(pickRgb);
        await driver.settingsLabelsFillName(name);
        await driver.settingsLabelsSubmitCreate();
        await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).toContain(name);
        const created = (await serverLabels(seed.workspaceSlug, projectId, session)).find((l) => l.name === name);
        expect(created?.color.toLowerCase()).toBe(pick.toLowerCase());
      });

      await test.step("a duplicate name toasts and keeps the form open", async () => {
        await driver.settingsLabelsOpenCreate();
        await driver.settingsLabelsFillName(name);
        await driver.settingsLabelsAttemptSubmit();
        await expect
          .poll(
            async () => {
              const toast = await driver.rulesLastToast();
              return toast ? `${toast.title} ${toast.message}` : "";
            },
            { timeout: 15_000 }
          )
          .toContain("Label already exists");
        expect(await driver.settingsLabelsFormVisible()).toBe(true);
        await driver.settingsLabelsCancelForm();
      });

      await test.step("the new label is offered by the issue labels picker", async () => {
        await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
        await driver.issueLabelsOpenPicker();
        await expect.poll(() => driver.issueLabelsOptionTexts(), { timeout: 15_000 }).toContain(name);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-227"], "label rename/recolor persists, lands on issues, and rolls back on failure"),
  { tag: specTags(["ISS-227"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 lble ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const label = await serverCreateLabel(seed.workspaceSlug, projectId, `${tag} beta`, "#FF6900", session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    await serverSetIssueLabels(seed.workspaceSlug, projectId, issue.id, [label.id], session);
    // Disjoint from the original so the row-text assertions can tell old from new.
    const renamed = `${tag} gamma`;
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);

      await test.step("a plain label offers only the edit action, pre-filled", async () => {
        await driver.settingsLabelsOpenRowMenu(label.name);
        expect(await driver.settingsLabelsMenuItems()).toEqual(["Edit label"]);
        await driver.settingsLabelsMenuPick("Edit label");
        // The form pre-fills through an effect after mount, so poll.
        await expect.poll(() => driver.settingsLabelsNameValue(), { timeout: 10_000 }).toBe(label.name);
        await expect.poll(() => driver.settingsLabelsDotColor(), { timeout: 10_000 }).toBe("rgb(255, 105, 0)");
      });

      await test.step("rename and recolor persist to the server", async () => {
        await driver.settingsLabelsFillName(renamed);
        await driver.settingsLabelsOpenColorPicker();
        await driver.settingsLabelsPickColor("#9900EF");
        await driver.settingsLabelsSubmitUpdate();
        await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).toContain(renamed);
        const updated = await serverLabelOrNull(seed.workspaceSlug, projectId, label.id, session);
        expect(updated?.name).toBe(renamed);
        expect(updated?.color.toLowerCase()).toBe("#9900ef");
      });

      await test.step("the rename lands on the carrying issue", async () => {
        await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
        const row = await driver.issueLabelsRowText();
        expect(row).toContain(renamed);
        expect(row).not.toContain(label.name);
        expect((await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).label_ids).toEqual([label.id]);
      });

      await test.step("a failed update toasts and rolls the row back", async () => {
        await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);
        await driver.settingsLabelsOpenRowMenu(renamed);
        await driver.settingsLabelsMenuPick("Edit label");
        await driver.settingsLabelsFillName(`${tag} ghost`);
        await serverCleanupLabel(seed.workspaceSlug, projectId, label.id, session);
        await driver.settingsLabelsAttemptSubmit();
        await expect
          .poll(
            async () => {
              const toast = await driver.rulesLastToast();
              return toast ? `${toast.title} ${toast.message}` : "";
            },
            { timeout: 15_000 }
          )
          .toContain("Error while updating the label");
        expect(await driver.settingsLabelsFormVisible()).toBe(true);
        await driver.settingsLabelsCancelForm();
        await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).toContain(renamed);
        expect(await serverLabelOrNull(seed.workspaceSlug, projectId, label.id, session)).toBe(null);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-228"], "label delete confirms, cancels cleanly, and toasts on failure"),
  { tag: specTags(["ISS-228"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 lbld ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const doomed = await serverCreateLabel(seed.workspaceSlug, projectId, `${tag} doomed`, "#EB144C", session);
    const control = await serverCreateLabel(seed.workspaceSlug, projectId, `${tag} control`, "#0693E3", session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);

      await test.step("the confirmation names the label and warns about removal", async () => {
        await driver.settingsLabelsDeleteViaTrash(doomed.name);
        const modal = await driver.settingsLabelsDeleteModalText();
        expect(modal).toContain(doomed.name);
        expect(modal).toContain("remove the label from all the work item");
      });

      await test.step("cancel leaves the label untouched", async () => {
        await driver.settingsLabelsDeleteCancel();
        expect(await driver.settingsLabelsDeleteModalText()).toBe("");
        await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).toContain(doomed.name);
        expect(await serverLabelOrNull(seed.workspaceSlug, projectId, doomed.id, session)).not.toBe(null);
      });

      await test.step("confirm deletes it from the list and the server", async () => {
        await driver.settingsLabelsDeleteViaTrash(doomed.name);
        await driver.settingsLabelsDeleteConfirm();
        await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).not.toContain(doomed.name);
        expect(await serverLabelOrNull(seed.workspaceSlug, projectId, doomed.id, session)).toBe(null);
        await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).toContain(control.name);
      });

      await test.step("a failed delete toasts and keeps the row", async () => {
        await driver.settingsLabelsDeleteViaTrash(control.name);
        await serverCleanupLabel(seed.workspaceSlug, projectId, control.id, session);
        await driver.settingsLabelsAttemptDeleteConfirm();
        await expect
          .poll(
            async () => {
              const toast = await driver.rulesLastToast();
              return toast ? `${toast.title} ${toast.message}` : "";
            },
            { timeout: 15_000 }
          )
          .toContain("Label could not be deleted. Please try again.");
        expect(await driver.settingsLabelsDeleteModalText()).not.toBe("");
        await driver.settingsLabelsDeleteCancel();
        await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).toContain(control.name);
        expect(await serverLabelOrNull(seed.workspaceSlug, projectId, control.id, session)).toBe(null);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
