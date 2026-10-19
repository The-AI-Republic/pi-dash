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
  editableFromDraft,
  editableFromIssue,
  editMembership,
  formFromRow,
  formFromSnapshot,
  hasEditMembership,
  isPatchEmpty,
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
      gitWorkBranch: "",
    });
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
    expect(set).toMatchObject({ assigned_pod_id: "pod-1", agent_executor: "local_runner" });

    const clearedForm = { ...form, execution: null };
    const clearedBaseline = { ...baselineFor(form) };
    expect(buildEditPayload(clearedForm, clearedBaseline)).toMatchObject({
      assigned_pod_id: null,
      agent_executor: null,
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
    });
    expect(hasEditMembership(membership)).toBe(false);
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
    });
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
    });
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
  it("refuses a form with no project", () => {
    expect(() => baselineFor(emptyCreateForm(null))).toThrow("An edit needs a project");
  });
});

describe("formFromSnapshot", () => {
  it("restores the fields a persisted snapshot carries", () => {
    const form = formFromSnapshot({ ...row(), extra: "ignored" }, PROJECT);
    expect(form).not.toBeNull();
    expect(form).toMatchObject({ name: "Existing row", stateId: STATE, moduleIds: [MODULE_A] });
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
});
