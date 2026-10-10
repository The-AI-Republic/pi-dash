// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-32): the basic inline pickers — state with
// rollback on a rejected save, priority, project-scoped labels, and
// project-member assignees. Rows: DRAFT-012, DRAFT-013, DRAFT-014,
// DRAFT-015. Green on apps/web first.
import { test, expect } from "../fixtures";
import {
  ROLE,
  createProjectViaApi,
  currentUser,
  parityProjectIdentifier,
  serverCreateDraft,
  serverCreateLabel,
  serverCreateState,
  serverDraftRecord,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { draftsHarness, draftsOpenAs, draftsSeat } from "./support";

test(
  specTitle(["DRAFT-012"], "state changes inline, persists, and rolls back on a rejected save"),
  { tag: specTags(["DRAFT-012"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d12");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D12 State ${harness.tag}`;
    const draft = await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });
    const doingId = await serverCreateState(workspaceSlug, projectId, "Doing", "started", owner.cookie);
    let initialName = "";

    await test.step("the new state renders immediately and persists", async () => {
      await draftsOpenAs(driver, harness);
      initialName = await driver.draftStateText(name);
      expect(initialName.length).toBeGreaterThan(0);
      await driver.draftOpenStatePicker(name);
      const options = await driver.pickerOptionTexts();
      expect(options).toContain("Doing");
      await driver.pickerPick("Doing");
      await expect.poll(() => driver.draftStateText(name), { timeout: 30_000 }).toBe("Doing");
      await expect
        .poll(async () => (await serverDraftRecord(workspaceSlug, draft.id, owner.cookie)).stateId, {
          timeout: 30_000,
        })
        .toBe(doingId);
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftStateText(name), { timeout: 60_000 }).toBe("Doing");
    });

    await test.step("a rejected save rolls the row back (silently — see the bug: scenario below)", async () => {
      try {
        await driver.draftFailNextPatch(500, 1500);
        await driver.draftOpenStatePicker(name);
        await driver.pickerPick(initialName);
        await expect.poll(() => driver.draftStateText(name), { timeout: 30_000 }).toBe(initialName);
        await expect.poll(() => driver.draftStateText(name), { timeout: 30_000 }).toBe("Doing");
        expect((await serverDraftRecord(workspaceSlug, draft.id, owner.cookie)).stateId).toBe(doingId);
      } finally {
        await driver.draftClearPatchFailure().catch(() => {});
      }
    });
  }
);

// The oracle rolls a rejected inline save back without any error
// notice; DRAFT-012 requires one. Pinned here until NEWFRONT-303 lands
// the fix.
test(
  specTitle(["DRAFT-012"], "bug: rejected inline save rolls back silently (NEWFRONT-303)"),
  { tag: specTags(["DRAFT-012"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d12bug");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D12 Silent ${harness.tag}`;
    const draft = await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });
    const doingId = await serverCreateState(workspaceSlug, projectId, "Doing", "started", owner.cookie);

    await test.step("a rejected save snaps back with no notice", async () => {
      await draftsOpenAs(driver, harness);
      const initialName = await driver.draftStateText(name);
      await driver.draftOpenStatePicker(name);
      await driver.pickerPick("Doing");
      await expect.poll(() => driver.draftStateText(name), { timeout: 30_000 }).toBe("Doing");
      try {
        await driver.draftFailNextPatch(500, 1500);
        await driver.draftOpenStatePicker(name);
        await driver.pickerPick(initialName);
        await expect.poll(() => driver.draftStateText(name), { timeout: 30_000 }).toBe(initialName);
        await expect.poll(() => driver.draftStateText(name), { timeout: 30_000 }).toBe("Doing");
        expect(await driver.toastText()).toBeNull();
        expect((await serverDraftRecord(workspaceSlug, draft.id, owner.cookie)).stateId).toBe(doingId);
      } finally {
        await driver.draftClearPatchFailure().catch(() => {});
      }
    });
  }
);

test(
  specTitle(["DRAFT-013"], "priority changes inline and persists"),
  { tag: specTags(["DRAFT-013"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d13");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D13 Priority ${harness.tag}`;
    const draft = await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });

    await test.step("the new priority renders immediately and persists", async () => {
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftPriorityText(name), { timeout: 60_000 }).toBe("None");
      await driver.draftOpenPriorityPicker(name);
      await driver.pickerPick("High");
      await expect.poll(() => driver.draftPriorityText(name), { timeout: 30_000 }).toBe("High");
      await expect
        .poll(async () => (await serverDraftRecord(workspaceSlug, draft.id, owner.cookie)).priority, {
          timeout: 30_000,
        })
        .toBe("high");
      await driver.draftOpenPriorityPicker(name);
      expect(await driver.draftPickerOptionSelected("High")).toBe(true);
      await driver.pickerPressEscape();
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftPriorityText(name), { timeout: 60_000 }).toBe("High");
    });
  }
);

test(
  specTitle(["DRAFT-014"], "labels change inline from the draft's own project"),
  { tag: specTags(["DRAFT-014"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d14");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D14 Labels ${harness.tag}`;
    const draft = await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });
    const mine = await serverCreateLabel(workspaceSlug, projectId, `Mine ${harness.tag}`, "#00aa55", owner.cookie);
    const otherProject = await createProjectViaApi(owner, workspaceSlug, {
      name: `Foreign ${harness.tag}`,
      identifier: parityProjectIdentifier("D14"),
    });
    const foreign = await serverCreateLabel(
      workspaceSlug,
      otherProject,
      `Foreign ${harness.tag}`,
      "#ff0000",
      owner.cookie
    );

    await test.step("only the draft's project labels are offered", async () => {
      await draftsOpenAs(driver, harness);
      await driver.draftOpenLabelsPicker(name);
      const options = await driver.pickerOptionTexts();
      expect(options).toContain(mine.name);
      expect(options).not.toContain(foreign.name);
      await driver.pickerPick(mine.name);
      await driver.pickerPressEscape();
    });

    await test.step("the chosen label renders and persists", async () => {
      await expect.poll(() => driver.draftLabelsText(name), { timeout: 30_000 }).toContain(mine.name);
      await expect
        .poll(async () => (await serverDraftRecord(workspaceSlug, draft.id, owner.cookie)).labelIds, {
          timeout: 30_000,
        })
        .toContain(mine.id);
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftLabelsText(name), { timeout: 60_000 }).toContain(mine.name);
    });
  }
);

test(
  specTitle(["DRAFT-015"], "assignees change inline from the draft's project members"),
  { tag: specTags(["DRAFT-015"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d15");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D15 Assignees ${harness.tag}`;
    const draft = await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });
    const member = await draftsSeat(harness, ROLE.MEMBER, "parity-d15-member", ROLE.MEMBER);
    const outsider = await draftsSeat(harness, ROLE.MEMBER, "parity-d15-outsider");
    const ownerName = (await currentUser(owner.cookie)).display_name ?? "";
    const memberName = (await currentUser(member.cookie)).display_name ?? "";
    const outsiderName = (await currentUser(outsider.cookie)).display_name ?? "";
    expect(ownerName.length).toBeGreaterThan(0);
    expect(memberName.length).toBeGreaterThan(0);

    await test.step("only project members are offered", async () => {
      await draftsOpenAs(driver, harness);
      await driver.draftOpenAssigneesPicker(name);
      const options = await driver.pickerOptionTexts();
      // Each option carries its avatar initial; the viewing user reads "You".
      const cleaned = options.map((option) =>
        option
          .split("\n")
          .map((part) => part.trim())
          .filter(Boolean)
          .at(-1)
      );
      expect(cleaned).toContain("You");
      expect(cleaned).toContain(memberName);
      expect(cleaned).not.toContain(outsiderName);
      await driver.pickerPick(memberName);
      await driver.pickerPressEscape();
    });

    await test.step("the chosen assignee renders and persists", async () => {
      await expect
        .poll(async () => (await serverDraftRecord(workspaceSlug, draft.id, owner.cookie)).assigneeIds, {
          timeout: 30_000,
        })
        .toContain(member.userId);
      expect(await driver.draftAssigneesText(name)).not.toBe("");
      await draftsOpenAs(driver, harness);
      await expect
        .poll(async () => (await serverDraftRecord(workspaceSlug, draft.id, owner.cookie)).assigneeIds, {
          timeout: 60_000,
        })
        .toContain(member.userId);
    });
  }
);
