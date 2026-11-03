// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edit rules for the modal (parity/specs/issues.md §11, ISS-124/ISS-126):
// how a row becomes the form, which fields the PATCH carries, how cycle
// and module bridges are diffed, and how a draft or publish input is
// shaped. These are pure rules, so every branch is driven directly.

import { describe, expect, it } from "vitest";

import type { IssuePatch } from "@pidash/api-client";

import { emptyCreateForm, type CreateFormState } from "./createForm.js";
import {
  baselineFor,
  buildEditPayload,
  buildPublishInput,
  changedCycleId,
  changedDraftProject,
  changedModuleIds,
  changedTypeId,
  editableFromDraft,
  editableFromIssue,
  editMembership,
  formFromRow,
  formFromSnapshot,
  hasEditMembership,
  isPatchEmpty,
  withStoredBranch,
  withStoredBranchBaseline,
  type EditableRow,
} from "./editForm.js";

const PROJECT = "11111111-1111-4111-8111-111111111111";
const STATE = "22222222-2222-4222-8222-222222222222";
const OTHER_STATE = "33333333-3333-4333-8333-333333333333";
const CYCLE = "44444444-4444-4444-8444-444444444444";
const OTHER_CYCLE = "55555555-5555-4555-8555-555555555555";
const MODULE_A = "66666666-6666-4666-8666-666666666666";
const MODULE_B = "77777777-7777-4777-8777-777777777777";
const PARENT = "88888888-8888-4888-8888-888888888888";

function row(overrides: Partial<EditableRow> = {}): EditableRow {
  return {
    id: "issue-1",
    name: "Existing row",
    description_html: "<p>stored body</p>",
    state_id: STATE,
    priority: "medium",
    assignee_ids: ["u-1"],
    label_ids: ["l-1"],
    start_date: "2026-02-01",
    target_date: "2026-02-09",
    cycle_id: CYCLE,
    module_ids: [MODULE_A],
    estimate_point: "e-1",
    parent_id: PARENT,
    git_work_branch: "team/stored",
    ...overrides,
  };
}

function unchanged(form: CreateFormState): IssuePatch {
  return buildEditPayload(form, baselineFor(form));
}

describe("formFromRow", () => {
  it("copies the row into the form and defaults the branch", () => {
    const form = formFromRow(row(), PROJECT);
    expect(form).toMatchObject({
      projectId: PROJECT,
      name: "Existing row",
      descriptionHtml: "<p>stored body</p>",
      stateId: STATE,
      priority: "medium",
      assigneeIds: ["u-1"],
      labelIds: ["l-1"],
      startDate: "2026-02-01",
      targetDate: "2026-02-09",
      cycleId: CYCLE,
      moduleIds: [MODULE_A],
      estimatePoint: "e-1",
      execution: null,
      parentId: PARENT,
      gitWorkBranch: "team/stored",
    });
  });

  it("defaults the branch when the row carries none", () => {
    expect(formFromRow(row({ git_work_branch: null }), PROJECT).gitWorkBranch).toBe("");
    expect(formFromRow(row({ git_work_branch: undefined }), PROJECT).gitWorkBranch).toBe("");
  });

  it("leaves the form untouched when no stored branch arrived", () => {
    const form = formFromRow(row(), PROJECT);
    expect(withStoredBranch(form, undefined)).toBe(form);
    expect(withStoredBranch(form, "team/late")).toEqual({ ...form, gitWorkBranch: "team/late" });
  });

  it("normalizes absent and empty optional fields to null", () => {
    const form = formFromRow(
      row({
        name: null,
        description_html: undefined,
        start_date: "",
        target_date: null,
        cycle_id: undefined,
        module_ids: undefined,
        estimate_point: undefined,
        priority: undefined as unknown as string,
      }),
      PROJECT
    );
    expect(form.name).toBe("");
    expect(form.descriptionHtml).toBe("");
    expect(form.startDate).toBeNull();
    expect(form.targetDate).toBeNull();
    expect(form.cycleId).toBeNull();
    expect(form.moduleIds).toEqual([]);
    expect(form.estimatePoint).toBeNull();
  });

  it("treats the no-estimate sentinel as no selection", () => {
    expect(formFromRow(row({ estimate_point: "none" }), PROJECT).estimatePoint).toBeNull();
    expect(formFromRow(row({ estimate_point: 3 }), PROJECT).estimatePoint).toBe("3");
  });

  it("copies list fields so the form never aliases the row", () => {
    const source = row();
    const form = formFromRow(source, PROJECT);
    form.assigneeIds.push("u-2");
    form.labelIds.push("l-2");
    form.moduleIds.push(MODULE_B);
    expect(source.assignee_ids).toEqual(["u-1"]);
    expect(source.label_ids).toEqual(["l-1"]);
    expect(source.module_ids).toEqual([MODULE_A]);
  });

  it("narrows a live issue and a draft to the same editable row", () => {
    expect(editableFromIssue(row() as never).id).toBe("issue-1");
    expect(editableFromDraft(row() as never).id).toBe("issue-1");
  });
});

describe("buildEditPayload", () => {
  it("sends nothing when the form still matches its baseline", () => {
    const patch = unchanged(formFromRow(row(), PROJECT));
    expect(patch).toEqual({});
    expect(isPatchEmpty(patch)).toBe(true);
  });

  it("sends only the fields that changed", () => {
    const form = formFromRow(row(), PROJECT);
    const baseline = baselineFor(form);
    form.name = "  Renamed  ";
    form.stateId = OTHER_STATE;
    form.priority = "urgent";
    const patch = buildEditPayload(form, baseline);
    expect(patch).toEqual({ name: "Renamed", state_id: OTHER_STATE, priority: "urgent" });
    expect(isPatchEmpty(patch)).toBe(false);
  });

  it("sends a cleared description", () => {
    const form = formFromRow(row(), PROJECT);
    const baseline = baselineFor(form);
    form.descriptionHtml = "<p></p>";
    expect(buildEditPayload(form, baseline)).toEqual({ description_html: "" });
  });

  it("compares assignees and labels as sets", () => {
    const form = formFromRow(row({ assignee_ids: ["u-1", "u-2"], label_ids: ["l-1", "l-2"] }), PROJECT);
    const baseline = baselineFor(form);
    form.assigneeIds = ["u-2", "u-1"];
    form.labelIds = ["l-2", "l-1"];
    expect(buildEditPayload(form, baseline)).toEqual({});
    form.assigneeIds = ["u-1"];
    expect(buildEditPayload(form, baseline)).toEqual({ assignee_ids: ["u-1"] });
  });

  it("sends dates, estimate, parent and branch only when they move", () => {
    const form = formFromRow(row(), PROJECT);
    const baseline = baselineFor(form);
    form.startDate = null;
    form.targetDate = "2026-03-01";
    form.estimatePoint = null;
    form.parentId = null;
    form.gitWorkBranch = "  team/feature  ";
    expect(buildEditPayload(form, baseline)).toEqual({
      start_date: null,
      target_date: "2026-03-01",
      estimate_point: null,
      parent_id: null,
      git_work_branch: "team/feature",
    });
  });

  it("sends an emptied branch so a saved one can be cleared (ISS-128)", () => {
    const form = formFromRow(row(), PROJECT);
    const baseline = { ...baselineFor(form), form: { ...baselineFor(form).form, gitWorkBranch: "old/branch" } };
    form.gitWorkBranch = "   ";
    expect(buildEditPayload(form, baseline)).toEqual({ git_work_branch: "" });
  });

  it("adopts the stored branch as the baseline a save diffs against (ISS-128)", () => {
    const form = formFromRow(row(), PROJECT);
    const baseline = baselineFor(form);
    form.gitWorkBranch = "mine/typed";
    expect(buildEditPayload(form, withStoredBranchBaseline(baseline, "team/stored")!)).toEqual({
      git_work_branch: "mine/typed",
    });
    const untouched = formFromRow({ ...row(), git_work_branch: "team/stored" }, PROJECT);
    expect(buildEditPayload(untouched, withStoredBranchBaseline(baseline, "team/stored")!)).toEqual({});
  });

  it("leaves a missing baseline missing", () => {
    expect(withStoredBranchBaseline(null, "team/stored")).toBeNull();
  });

  it("keeps an untouched empty branch out of the patch", () => {
    const form = formFromRow(row(), PROJECT);
    expect(buildEditPayload(form, baselineFor(form))).toEqual({});
  });

  it("sends a description made only of an image or a mention (ISS-127)", () => {
    for (const body of [
      '<p><img src="https://s3.test/a.png"></p>',
      '<p><mention-component id="u-1"></mention-component></p>',
    ]) {
      const form = formFromRow(row({ description_html: "" }), PROJECT);
      const baseline = baselineFor(form);
      form.descriptionHtml = body;
      expect(buildEditPayload(form, baseline)).toEqual({ description_html: body });
    }
  });

  it("sends an emptied description rather than dropping the change", () => {
    const form = formFromRow(row(), PROJECT);
    const baseline = baselineFor(form);
    form.descriptionHtml = "<p></p>";
    expect(buildEditPayload(form, baseline)).toEqual({ description_html: "" });
  });

  it("pairs the execution target columns on a change and clears both", () => {
    const form = formFromRow(row(), PROJECT);
    const target = { type: "pod" as const, podId: "pod-1" };
    const baseline = baselineFor(form);
    form.execution = target;
    const set = buildEditPayload(form, baseline);
    expect(set).toMatchObject({
      assigned_pod_id: "pod-1",
      assigned_runner_id: null,
      agent_executor: "local_runner",
    });

    const clearedForm = { ...form, execution: null };
    const clearedBaseline = { ...baselineFor(form) };
    expect(buildEditPayload(clearedForm, clearedBaseline)).toMatchObject({
      assigned_pod_id: null,
      assigned_runner_id: null,
      agent_executor: null,
    });
  });

  it("writes the runner pin when the execution moves to a runner", () => {
    const form = formFromRow(row(), PROJECT);
    form.execution = { type: "pod", podId: "pod-1" };
    const baseline = baselineFor(form);
    form.execution = { type: "runner", runnerId: "runner-1", podId: "pod-1" };
    expect(buildEditPayload(form, baseline)).toMatchObject({
      assigned_pod_id: "pod-1",
      assigned_runner_id: "runner-1",
      agent_executor: "local_runner",
    });
  });

  it("leaves the execution target alone when only one column was re-picked", () => {
    const form = formFromRow(row(), PROJECT);
    const baseline = baselineFor(form);
    form.execution = { type: "pod", podId: "pod-1" };
    baseline.form = { ...baseline.form, execution: { type: "pod", podId: "pod-1" } };
    expect(buildEditPayload(form, baseline)).toEqual({});
  });
});

describe("membership diffing", () => {
  it("owes nothing when the memberships are unchanged", () => {
    const form = formFromRow(row(), PROJECT);
    const membership = editMembership(form, baselineFor(form));
    expect(membership).toEqual({
      removedCycleId: null,
      addedCycleId: null,
      addedModuleIds: [],
      removedModuleIds: [],
      cycleId: CYCLE,
      moduleIds: [MODULE_A],
    });
    expect(hasEditMembership(membership)).toBe(false);
    expect(changedCycleId(membership)).toBeUndefined();
    expect(changedModuleIds(membership)).toBeUndefined();
  });

  it("swaps one cycle for another and diffs modules by id", () => {
    const form = formFromRow(row({ module_ids: [MODULE_A, MODULE_B] }), PROJECT);
    const baseline = baselineFor(form);
    form.cycleId = OTHER_CYCLE;
    form.moduleIds = [MODULE_B, "m-new"];
    expect(editMembership(form, baseline)).toEqual({
      removedCycleId: CYCLE,
      addedCycleId: OTHER_CYCLE,
      addedModuleIds: ["m-new"],
      removedModuleIds: [MODULE_A],
      cycleId: OTHER_CYCLE,
      moduleIds: [MODULE_B, "m-new"],
    });
    const membership = editMembership(form, baseline);
    // A draft stores the complete set, not the diff (ISS-124/ISS-126).
    expect(changedCycleId(membership)).toBe(OTHER_CYCLE);
    expect(changedModuleIds(membership)).toEqual([MODULE_B, "m-new"]);
  });

  it("clears a cycle without adding one", () => {
    const form = formFromRow(row(), PROJECT);
    const baseline = baselineFor(form);
    form.cycleId = null;
    expect(editMembership(form, baseline)).toEqual({
      removedCycleId: CYCLE,
      addedCycleId: null,
      addedModuleIds: [],
      removedModuleIds: [],
      cycleId: null,
      moduleIds: [MODULE_A],
    });
    expect(changedCycleId(editMembership(form, baseline))).toBeNull();
    expect(hasEditMembership(editMembership(form, baseline))).toBe(true);
  });

  it("adds a cycle to a row that had none", () => {
    const form = formFromRow(row({ cycle_id: null }), PROJECT);
    const baseline = baselineFor(form);
    form.cycleId = CYCLE;
    expect(editMembership(form, baseline)).toMatchObject({ removedCycleId: null, addedCycleId: CYCLE });
  });
});

describe("baselineFor", () => {
  it("keeps a project-less draft's null project so a pick can be stored (ISS-130)", () => {
    expect(baselineFor(emptyCreateForm(null)).projectId).toBeNull();
  });
});

describe("changedDraftProject", () => {
  it("reports a picked project for a draft with none stored", () => {
    const form = emptyCreateForm(null);
    const baseline = baselineFor(form);
    form.projectId = PROJECT;
    expect(changedDraftProject(form, baseline)).toBe(PROJECT);
  });

  it("reports a project moved to a different one", () => {
    const form = formFromRow(row(), PROJECT);
    const baseline = baselineFor(form);
    form.projectId = "22222222-2222-4222-8222-222222222222";
    expect(changedDraftProject(form, baseline)).toBe("22222222-2222-4222-8222-222222222222");
  });

  it("reports nothing when the project did not move", () => {
    const form = formFromRow(row(), PROJECT);
    expect(changedDraftProject(form, baselineFor(form))).toBeUndefined();
  });
});

describe("changedTypeId", () => {
  it("sends nothing when the pick never moved (ISS-129)", () => {
    expect(changedTypeId("type-1", "type-1")).toBeUndefined();
    expect(changedTypeId(null, null)).toBeUndefined();
  });

  it("sends the pick when it moved off the stored row (ISS-129)", () => {
    expect(changedTypeId("type-2", "type-1")).toBe("type-2");
    expect(changedTypeId("type-1", null)).toBe("type-1");
  });

  it("sends null when cleared back to No type (ISS-129)", () => {
    // The server clears the row; omitting the key would silently keep
    // the stored type while the picker shows none.
    expect(changedTypeId(null, "type-1")).toBeNull();
  });
});

describe("formFromSnapshot", () => {
  it("restores the fields a persisted snapshot carries", () => {
    const form = formFromSnapshot({ ...row(), extra: "ignored" }, PROJECT);
    expect(form).not.toBeNull();
    expect(form).toMatchObject({
      name: "Existing row",
      stateId: STATE,
      moduleIds: [MODULE_A],
      gitWorkBranch: "team/stored",
    });
  });

  it("carries a stored branch from a snapshot and drops a wrongly typed one", () => {
    expect(formFromSnapshot({ ...row(), git_work_branch: "team/saved" }, PROJECT)?.gitWorkBranch).toBe("team/saved");
    expect(formFromSnapshot({ ...row(), git_work_branch: 7 }, PROJECT)?.gitWorkBranch).toBe("");
  });

  it("rejects a snapshot that is not a row", () => {
    expect(formFromSnapshot(null, PROJECT)).toBeNull();
    expect(formFromSnapshot("text", PROJECT)).toBeNull();
    expect(formFromSnapshot({ name: "no id" }, PROJECT)).toBeNull();
  });

  it("opens a project-less draft without inventing a project", () => {
    const form = formFromSnapshot(row(), null);
    expect(form).toMatchObject({ projectId: null, name: "Existing row" });
  });

  it("falls back to safe defaults for wrongly typed fields", () => {
    const form = formFromSnapshot(
      { id: "draft-1", name: 7, priority: 3, assignee_ids: ["a", 4], module_ids: "no", start_date: 9 },
      PROJECT
    );
    expect(form).toMatchObject({
      name: "",
      priority: "none",
      assigneeIds: ["a"],
      moduleIds: [],
      startDate: null,
    });
  });
});

describe("buildPublishInput", () => {
  it("carries every stored value and omits empty ones", () => {
    const form = emptyCreateForm(PROJECT);
    form.name = "  Publish me  ";
    form.descriptionHtml = "<p>body</p>";
    form.priority = "high";
    form.assigneeIds = ["u-1"];
    expect(buildPublishInput(form)).toEqual({
      name: "Publish me",
      description_html: "<p>body</p>",
      project_id: PROJECT,
      priority: "high",
      assignee_ids: ["u-1"],
    });
  });

  it("stays minimal for a blank draft", () => {
    const form = emptyCreateForm(null);
    form.name = "Only a name";
    expect(buildPublishInput(form)).toEqual({ name: "Only a name" });
  });

  it("keeps the draft's cycle, modules and estimate (ISS-130)", () => {
    // The promote endpoint files the issue in the named cycle and
    // modules and stores its estimate; leaving them out loses the
    // draft's placement (ISS-130).
    const form = emptyCreateForm(PROJECT);
    form.name = "Placed";
    form.cycleId = "c-1";
    form.moduleIds = ["m-1", "m-2"];
    form.estimatePoint = "e-1";
    expect(buildPublishInput(form)).toEqual({
      name: "Placed",
      project_id: PROJECT,
      cycle_id: "c-1",
      module_ids: ["m-1", "m-2"],
      estimate_point: "e-1",
    });
  });
});
