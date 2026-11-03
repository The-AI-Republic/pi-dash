// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Epic dialog shaping (ISS-232): the blank create, the draft from a row,
// the PATCH and create payloads, and the name rule.
import type { Issue } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  DEFAULT_EPIC_PRIORITY,
  canSubmitEpic,
  epicCreateOf,
  epicDraftFrom,
  epicNameError,
  epicPatchOf,
  epicTitle,
  type EpicDialogValues,
} from "./epicModel.js";

function epic(overrides: Partial<Issue> = {}): Issue {
  return {
    id: "623e4567-e89b-12d3-a456-426614174010",
    name: "Ship the launch",
    description_html: "<p>Plan</p>",
    state_id: "723e4567-e89b-12d3-a456-426614174011",
    priority: "high",
    assignee_ids: ["823e4567-e89b-12d3-a456-426614174012"],
    label_ids: ["923e4567-e89b-12d3-a456-426614174013"],
    start_date: "2026-01-01",
    target_date: "2026-02-01",
    ...overrides,
  } as Issue;
}

const FILLED: EpicDialogValues = {
  name: "  Ship the launch  ",
  descriptionHtml: "<p>Plan</p>",
  stateId: "723e4567-e89b-12d3-a456-426614174011",
  priority: "high",
  assigneeIds: ["823e4567-e89b-12d3-a456-426614174012"],
  labelIds: ["923e4567-e89b-12d3-a456-426614174013"],
  startDate: "2026-01-01",
  targetDate: "2026-02-01",
};

describe("epicDraftFrom", () => {
  it("starts a create blank, with the server's default priority", () => {
    expect(epicDraftFrom(undefined)).toEqual({
      name: "",
      descriptionHtml: "",
      stateId: null,
      priority: DEFAULT_EPIC_PRIORITY,
      assigneeIds: [],
      labelIds: [],
      startDate: null,
      targetDate: null,
    });
  });

  it("copies the row being edited, including both dates", () => {
    expect(epicDraftFrom(epic())).toEqual({ ...FILLED, name: "Ship the launch" });
  });

  it("reads an absent optional field as unset rather than undefined", () => {
    const draft = epicDraftFrom(
      epic({ start_date: undefined, target_date: null, state_id: null, assignee_ids: [], label_ids: [] })
    );
    expect(draft.startDate).toBeNull();
    expect(draft.targetDate).toBeNull();
    expect(draft.stateId).toBeNull();
    expect(draft.assigneeIds).toEqual([]);
  });
});

describe("epicPatchOf", () => {
  it("sends the trimmed name and every editable field", () => {
    expect(epicPatchOf(FILLED)).toEqual({
      name: "Ship the launch",
      description_html: "<p>Plan</p>",
      state_id: "723e4567-e89b-12d3-a456-426614174011",
      priority: "high",
      assignee_ids: FILLED.assigneeIds,
      label_ids: FILLED.labelIds,
      start_date: "2026-01-01",
      target_date: "2026-02-01",
    });
  });
});

describe("epicCreateOf", () => {
  it("sends the trimmed name and omits an unset state", () => {
    const body = epicCreateOf({ ...FILLED, stateId: null });
    expect(body).toEqual({
      name: "Ship the launch",
      description_html: "<p>Plan</p>",
      priority: "high",
      assignee_ids: FILLED.assigneeIds,
      label_ids: FILLED.labelIds,
      start_date: "2026-01-01",
      target_date: "2026-02-01",
    });
    expect("state_id" in body).toBe(false);
  });

  it("carries the chosen state", () => {
    expect(epicCreateOf(FILLED).state_id).toBe("723e4567-e89b-12d3-a456-426614174011");
  });
});

describe("name rule", () => {
  it("refuses a blank or whitespace-only name", () => {
    expect(canSubmitEpic({ ...FILLED, name: "" })).toBe(false);
    expect(canSubmitEpic({ ...FILLED, name: "   " })).toBe(false);
    expect(epicNameError({ ...FILLED, name: "  " })).toBe("Give the epic a name.");
  });

  it("accepts a named epic and clears the error", () => {
    expect(canSubmitEpic(FILLED)).toBe(true);
    expect(epicNameError(FILLED)).toBeUndefined();
  });
});

describe("epicTitle", () => {
  it("names the mode", () => {
    expect(epicTitle(false)).toBe("Create epic");
    expect(epicTitle(true)).toBe("Edit epic");
  });
});
