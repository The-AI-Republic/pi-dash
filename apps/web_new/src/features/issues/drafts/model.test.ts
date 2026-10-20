// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Draft shaping rules (parity/specs/issues.md §12, ISS-137–141).

import { describe, expect, it } from "vitest";

import type { DraftIssue } from "@pidash/api-client";

import {
  asPriority,
  assigneePatch,
  canDeleteDraft,
  copyDraftInput,
  copyDraftTitle,
  cyclePatch,
  DRAFT_COPY_SUFFIX,
  draftSnapshot,
  draftStateId,
  estimatePatch,
  isDraftOverdue,
  labelPatch,
  modulePatch,
  priorityPatch,
  startDatePatch,
  statePatch,
  targetDatePatch,
} from "./model.js";

function draft(overrides: Partial<DraftIssue> = {}): DraftIssue {
  return {
    id: "draft-1",
    name: "Half written",
    state_id: "state-1",
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    start_date: null,
    target_date: null,
    project_id: "project-1",
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: "user-1",
    updated_by: null,
    type_id: null,
    description_html: "<p>body</p>",
    ...overrides,
  } as DraftIssue;
}

describe("draftSnapshot", () => {
  it("projects the stored row into the modal's editable shape", () => {
    const row = draft({
      estimate_point: 3,
      start_date: "2026-02-01",
      target_date: "2026-02-02",
      cycle_id: "cycle-1",
      module_ids: ["module-1"],
      label_ids: ["label-1"],
      assignee_ids: ["user-2"],
      parent_id: "parent-1",
      priority: "high",
    });
    expect(draftSnapshot(row)).toEqual({
      id: "draft-1",
      name: "Half written",
      description_html: "<p>body</p>",
      state_id: "state-1",
      priority: "high",
      assignee_ids: ["user-2"],
      label_ids: ["label-1"],
      start_date: "2026-02-01",
      target_date: "2026-02-02",
      cycle_id: "cycle-1",
      module_ids: ["module-1"],
      estimate_point: 3,
      parent_id: "parent-1",
    });
  });

  it("copies the id lists so a later pick cannot mutate the query row", () => {
    const row = draft({ label_ids: ["label-1"], module_ids: ["module-1"], assignee_ids: ["user-2"] });
    const snapshot = draftSnapshot(row);
    snapshot.label_ids.push("label-2");
    snapshot.module_ids.push("module-2");
    snapshot.assignee_ids.push("user-3");
    expect(row.label_ids).toEqual(["label-1"]);
    expect(row.module_ids).toEqual(["module-1"]);
    expect(row.assignee_ids).toEqual(["user-2"]);
  });

  it("keeps absent optional dates and estimates as null", () => {
    const row = draft();
    delete (row as { start_date?: string | null }).start_date;
    delete (row as { target_date?: string | null }).target_date;
    delete (row as { estimate_point?: string | number | null }).estimate_point;
    expect(draftSnapshot(row)).toMatchObject({ start_date: null, target_date: null, estimate_point: null });
  });
});

describe("copyDraftInput", () => {
  it("suffixes the name and carries every set field", () => {
    const row = draft({
      name: "Plan",
      project_id: "project-1",
      estimate_point: "point-1",
      start_date: "2026-02-01",
      target_date: "2026-02-02",
      cycle_id: "cycle-1",
      module_ids: ["module-1"],
      label_ids: ["label-1"],
      assignee_ids: ["user-2"],
      priority: "urgent",
      parent_id: "parent-1",
    });
    expect(copyDraftInput(row)).toEqual({
      name: `Plan${DRAFT_COPY_SUFFIX}`,
      description_html: "<p>body</p>",
      project_id: "project-1",
      state_id: "state-1",
      priority: "urgent",
      assignee_ids: ["user-2"],
      label_ids: ["label-1"],
      start_date: "2026-02-01",
      target_date: "2026-02-02",
      cycle_id: "cycle-1",
      module_ids: ["module-1"],
      estimate_point: "point-1",
      parent_id: "parent-1",
    });
  });

  it("leaves unset fields off the payload", () => {
    const row = draft();
    delete (row as { start_date?: string | null }).start_date;
    delete (row as { target_date?: string | null }).target_date;
    delete (row as { estimate_point?: string | number | null }).estimate_point;
    expect(copyDraftInput(row)).toEqual({
      name: `Half written${DRAFT_COPY_SUFFIX}`,
      description_html: "<p>body</p>",
      project_id: "project-1",
      state_id: "state-1",
    });
  });

  it("copies a project-less, unnamed draft as a bare suffixed draft", () => {
    const input = copyDraftInput(
      draft({ name: null, project_id: null, state_id: null, priority: "none", description_html: "" })
    );
    expect(input.name).toBe(DRAFT_COPY_SUFFIX.trim());
    expect(input.description_html).toBeUndefined();
    expect(input.project_id).toBeUndefined();
    expect(input.state_id).toBeUndefined();
    expect(input.priority).toBeUndefined();
  });

  it("trims the suffix away from an unnamed copy's title", () => {
    expect(copyDraftTitle(draft({ name: null }))).toBe(DRAFT_COPY_SUFFIX);
  });

  it("carries a numeric estimate as its point id string", () => {
    expect(copyDraftInput(draft({ estimate_point: 5 })).estimate_point).toBe("5");
  });
});

describe("draftStateId", () => {
  it("reads the draft's own state over the project default", () => {
    expect(draftStateId({ state_id: "state-1" }, { default_state: "default-1" })).toBe("state-1");
  });

  it("falls back to the project default while the draft holds none", () => {
    expect(draftStateId({ state_id: null }, { default_state: "default-1" })).toBe("default-1");
  });

  it("reads nothing without a draft state or a project", () => {
    expect(draftStateId({ state_id: null }, { default_state: null })).toBeNull();
    expect(draftStateId({ state_id: null }, undefined)).toBeNull();
  });
});

describe("asPriority", () => {
  it("accepts a picker priority and falls back to none", () => {
    expect(asPriority("urgent")).toBe("urgent");
    expect(asPriority("from-a-future-server")).toBe("none");
  });
});

describe("canDeleteDraft", () => {
  it("allows the creator regardless of project standing", () => {
    expect(canDeleteDraft({ created_by: "user-1" }, { meId: "user-1", projectRole: null })).toBe(true);
  });

  it("allows a project admin who did not create the draft", () => {
    expect(canDeleteDraft({ created_by: "someone" }, { meId: "user-1", projectRole: 20 })).toBe(true);
  });

  it("refuses a member, a project-less draft, and an unknown viewer", () => {
    expect(canDeleteDraft({ created_by: "someone" }, { meId: "user-1", projectRole: 15 })).toBe(false);
    expect(canDeleteDraft({ created_by: "someone" }, { meId: "user-1", projectRole: null })).toBe(false);
    expect(canDeleteDraft({ created_by: null }, { meId: null, projectRole: null })).toBe(false);
    expect(canDeleteDraft({ created_by: null }, { meId: null, projectRole: 20 })).toBe(true);
  });
});

describe("isDraftOverdue", () => {
  it("flags a due day strictly before today", () => {
    expect(isDraftOverdue("2026-02-01", "unstarted", "2026-02-02")).toBe(true);
  });

  it("never flags today or a later day", () => {
    expect(isDraftOverdue("2026-02-02", "started", "2026-02-02")).toBe(false);
    expect(isDraftOverdue("2026-02-03", "started", "2026-02-02")).toBe(false);
  });

  it("never flags finished work", () => {
    expect(isDraftOverdue("2026-01-01", "completed", "2026-02-02")).toBe(false);
    expect(isDraftOverdue("2026-01-01", "cancelled", "2026-02-02")).toBe(false);
  });

  it("reads plain without a due day or a resolved state", () => {
    expect(isDraftOverdue(null, "started", "2026-02-02")).toBe(false);
    expect(isDraftOverdue("2026-01-01", null, "2026-02-02")).toBe(false);
    expect(isDraftOverdue("2026-01-01", undefined, "2026-02-02")).toBe(false);
  });

  it("compares only the day part of a timestamp", () => {
    expect(isDraftOverdue("2026-02-01T23:00:00Z", "started", "2026-02-02")).toBe(true);
  });
});

describe("field patches", () => {
  it("each pick writes only its own field", () => {
    expect(statePatch("state-2")).toEqual({ state_id: "state-2" });
    expect(priorityPatch("low")).toEqual({ priority: "low" });
    expect(labelPatch(["label-1"])).toEqual({ label_ids: ["label-1"] });
    expect(assigneePatch(["user-2"])).toEqual({ assignee_ids: ["user-2"] });
    expect(startDatePatch("2026-02-01")).toEqual({ start_date: "2026-02-01" });
    expect(targetDatePatch(null)).toEqual({ target_date: null });
    expect(cyclePatch(null)).toEqual({ cycle_id: null });
    expect(modulePatch(["module-1"])).toEqual({ module_ids: ["module-1"] });
    expect(estimatePatch("point-1")).toEqual({ estimate_point: "point-1" });
  });

  it("copies the id lists so the caller's array is not shared", () => {
    const labels = ["label-1"];
    const patch = labelPatch(labels);
    labels.push("label-2");
    expect(patch.label_ids).toEqual(["label-1"]);
  });
});
