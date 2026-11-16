// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The module dialog model (parity/specs/modules.md §2.3, §3.2, §7.1–§7.5):
// the title rule, the defaults and prefill, the two write bodies, the
// pinned keyboard sequence (bug NEWFRONT-330), the projects a create may
// target, the failure readings and the notice copy.

import type { ProjectList } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  creatableModuleProjectIds,
  emptyModuleForm,
  isModuleDeletePermissionError,
  MODULE_CREATE_HEADING,
  MODULE_CREATE_SUBMIT,
  MODULE_DELETE_HEADING,
  MODULE_FAILURE_BODY,
  MODULE_FAILURE_TITLE,
  MODULE_FORM_TAB_ORDER,
  MODULE_SUCCESS_TITLE,
  MODULE_TITLE_MAX,
  MODULE_UPDATE_HEADING,
  MODULE_UPDATE_SUBMIT,
  moduleCreateNotice,
  moduleCreatePayload,
  moduleDeleteBody,
  moduleDeleteLanding,
  moduleDeleteNotice,
  moduleDialogHeading,
  moduleFailureBody,
  moduleFormFromRecord,
  moduleSubmitLabel,
  moduleTabOrderSequence,
  moduleTargetProjectId,
  moduleTitleError,
  moduleUpdateNotice,
  moduleUpdatePayload,
  saveDialogDismisses,
  type ModuleFormState,
} from "./moduleForm.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

function form(overrides: Partial<ModuleFormState> = {}): ModuleFormState {
  return { ...emptyModuleForm(PROJECT_ID), ...overrides };
}

function projectRow(overrides: Record<string, unknown>): Record<string, unknown> {
  return {
    id: PROJECT_ID,
    name: "Website relaunch",
    identifier: "WEB",
    sort_order: null,
    logo_props: {},
    member_role: 15,
    intake_count: 0,
    archived_at: null,
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

describe("moduleTitleError", () => {
  it("requires a non-blank title", () => {
    expect(moduleTitleError("")).toBe("Title is required");
    expect(moduleTitleError("   ")).toBe("Title is required");
    expect(moduleTitleError("\n\t ")).toBe("Title is required");
  });

  it("accepts the boundary length and refuses one more character", () => {
    expect(moduleTitleError("x".repeat(MODULE_TITLE_MAX))).toBeNull();
    expect(moduleTitleError("x".repeat(MODULE_TITLE_MAX + 1))).toBe("Title must be 255 characters or fewer");
  });

  it("accepts an ordinary title", () => {
    expect(moduleTitleError("Delivery")).toBeNull();
  });
});

describe("emptyModuleForm", () => {
  it("starts blank with Backlog, no people and no dates", () => {
    expect(emptyModuleForm(PROJECT_ID)).toEqual({
      projectId: PROJECT_ID,
      title: "",
      description: "",
      range: { start: null, end: null },
      status: "backlog",
      leadId: null,
      memberIds: [],
    });
  });

  it("mints a fresh range and member list per visit", () => {
    const first = emptyModuleForm(PROJECT_ID);
    const second = emptyModuleForm(PROJECT_ID);
    expect(first.range).not.toBe(second.range);
    expect(first.memberIds).not.toBe(second.memberIds);
  });
});

describe("moduleFormFromRecord", () => {
  it("prefills every field from the record and copies the member list", () => {
    const members = ["423e4567-e89b-12d3-a456-426614174003"];
    const state = moduleFormFromRecord(
      {
        id: "323e4567-e89b-12d3-a456-426614174002",
        name: "Prefill Me",
        description: "Prefill description",
        start_date: "2026-10-01",
        target_date: "2026-10-31",
        status: "planned",
        lead_id: "423e4567-e89b-12d3-a456-426614174003",
        member_ids: members,
      },
      PROJECT_ID
    );
    expect(state).toEqual({
      projectId: PROJECT_ID,
      title: "Prefill Me",
      description: "Prefill description",
      range: { start: "2026-10-01", end: "2026-10-31" },
      status: "planned",
      leadId: "423e4567-e89b-12d3-a456-426614174003",
      memberIds: members,
    });
    expect(state.memberIds).not.toBe(members);
  });

  it("keeps unset dates and people unset", () => {
    const state = moduleFormFromRecord(
      {
        id: "323e4567-e89b-12d3-a456-426614174002",
        name: "Plain",
        description: "",
        start_date: null,
        target_date: null,
        status: "backlog",
        lead_id: null,
        member_ids: [],
      },
      PROJECT_ID
    );
    expect(state.range).toEqual({ start: null, end: null });
    expect(state.leadId).toBeNull();
    expect(state.memberIds).toEqual([]);
  });
});

describe("moduleCreatePayload", () => {
  it("trims the title and sends explicit null dates", () => {
    expect(moduleCreatePayload(form({ title: "  Delivery  " }))).toEqual({
      name: "Delivery",
      description: "",
      start_date: null,
      target_date: null,
      status: "backlog",
      lead_id: null,
      member_ids: [],
    });
  });

  it("sends every entered field, copying the member list", () => {
    const members = ["423e4567-e89b-12d3-a456-426614174003"];
    const payload = moduleCreatePayload(
      form({
        title: "Full Module",
        description: "Line one\nLine two",
        range: { start: "2026-11-01", end: "2026-11-15" },
        status: "in-progress",
        leadId: members[0]!,
        memberIds: members,
      })
    );
    expect(payload).toEqual({
      name: "Full Module",
      description: "Line one\nLine two",
      start_date: "2026-11-01",
      target_date: "2026-11-15",
      status: "in-progress",
      lead_id: members[0],
      member_ids: members,
    });
    expect(payload.member_ids).not.toBe(members);
  });

  it("never sends the target project inside the body", () => {
    expect(moduleCreatePayload(form())).not.toHaveProperty("projectId");
    expect(moduleCreatePayload(form())).not.toHaveProperty("project_id");
  });
});

describe("moduleUpdatePayload", () => {
  it("writes the same fields through the partial path", () => {
    const state = form({ title: "Edited", status: "paused" });
    expect(moduleUpdatePayload(state)).toEqual(moduleCreatePayload(state));
  });
});

describe("moduleTargetProjectId", () => {
  it("reads the picked project, which create may retarget", () => {
    expect(moduleTargetProjectId(form({ projectId: "other" }))).toBe("other");
  });
});

describe("dialog copy", () => {
  it("pins the headings and submit labels per mode", () => {
    expect(moduleDialogHeading("create")).toBe(MODULE_CREATE_HEADING);
    expect(moduleDialogHeading("update")).toBe(MODULE_UPDATE_HEADING);
    expect(MODULE_CREATE_HEADING).toBe("Create module");
    expect(MODULE_UPDATE_HEADING).toBe("Update module");
    expect(MODULE_DELETE_HEADING).toBe("Delete module");
  });

  it("swaps the submit label while a write flies", () => {
    expect(moduleSubmitLabel("create", false)).toBe(MODULE_CREATE_SUBMIT);
    expect(moduleSubmitLabel("update", false)).toBe(MODULE_UPDATE_SUBMIT);
    expect(moduleSubmitLabel("create", true)).toBe("Creating module…");
    expect(moduleSubmitLabel("update", true)).toBe("Saving module…");
  });

  it("pins the outcome titles the oracle reads", () => {
    expect(MODULE_SUCCESS_TITLE).toBe("Success!");
    expect(MODULE_FAILURE_TITLE).toBe("Error!");
  });

  it("names the module in the create and delete notices", () => {
    expect(moduleCreateNotice("Delivery")).toContain("Delivery");
    expect(moduleDeleteNotice("Delivery")).toContain("Delivery");
    expect(moduleUpdateNotice()).toBe("Module updated successfully.");
  });

  it("names the module and states the loss in the delete body", () => {
    const body = moduleDeleteBody("Delete Me");
    expect(body).toContain("Delete Me");
    expect(body).toContain("permanently removed");
    expect(body).toContain("cannot be undone");
  });
});

describe("moduleTabOrderSequence", () => {
  it("pins title through submit with the date control third", () => {
    // Bug NEWFRONT-330: the old control carries no index, so date reads
    // null and lands after Submit. The intended sequence is 1..8.
    expect(moduleTabOrderSequence()).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
    expect(MODULE_FORM_TAB_ORDER.dateRange).toBe(3);
    expect(MODULE_FORM_TAB_ORDER.title).toBe(1);
    expect(MODULE_FORM_TAB_ORDER.submit).toBe(8);
  });
});

describe("moduleDeleteLanding", () => {
  it("returns to the plain list with an empty search so no peek survives", () => {
    expect(moduleDeleteLanding("acme", "p1")).toEqual({
      to: "/acme/projects/p1/modules/",
      search: {},
      replace: true,
    });
  });
});

describe("saveDialogDismisses", () => {
  it("lets Escape, Cancel and the close control through while a write flies", () => {
    expect(saveDialogDismisses(true, "escape-key")).toBe(true);
    expect(saveDialogDismisses(true, "close-press")).toBe(true);
    expect(saveDialogDismisses(true, "cancel-press")).toBe(true);
  });

  it("swallows an outside press only while a write is in flight", () => {
    expect(saveDialogDismisses(true, "outside-press")).toBe(false);
    expect(saveDialogDismisses(false, "outside-press")).toBe(true);
  });
});

describe("creatableModuleProjectIds", () => {
  const list = (rows: Record<string, unknown>[]): ProjectList => rows as unknown as ProjectList;

  it("keeps projects with modules on and at least member standing", () => {
    expect(
      creatableModuleProjectIds(
        list([
          projectRow({ id: "a", member_role: 20 }),
          projectRow({ id: "b", member_role: 15 }),
          projectRow({ id: "c", member_role: 10 }),
        ])
      )
    ).toEqual(["a", "b"]);
  });

  it("drops projects with modules off, archived, or no role", () => {
    expect(
      creatableModuleProjectIds(
        list([
          projectRow({ id: "off", module_view: false }),
          projectRow({ id: "archived", archived_at: "2026-01-02T03:04:05Z" }),
          projectRow({ id: "norole", member_role: null }),
        ])
      )
    ).toEqual([]);
  });

  it("keeps the server's order", () => {
    expect(
      creatableModuleProjectIds(list([projectRow({ id: "z" }), projectRow({ id: "a" }), projectRow({ id: "m" })]))
    ).toEqual(["z", "a", "m"]);
  });

  it("reads an empty list as nothing to offer", () => {
    expect(creatableModuleProjectIds(list([]))).toEqual([]);
  });
});

describe("isModuleDeletePermissionError", () => {
  it("recognises the server's 403 and nothing else", () => {
    expect(isModuleDeletePermissionError({ status: 403 })).toBe(true);
    expect(isModuleDeletePermissionError({ status: 400 })).toBe(false);
    expect(isModuleDeletePermissionError({ status: "403" })).toBe(false);
    expect(isModuleDeletePermissionError(null)).toBe(false);
    expect(isModuleDeletePermissionError("403")).toBe(false);
  });
});

describe("moduleFailureBody", () => {
  it("prefers a readable detail sentence", () => {
    expect(moduleFailureBody({ detail: "  Module with this name already exists  " })).toBe(
      "Module with this name already exists"
    );
  });

  it("falls back to the error message, then the fixed sentence", () => {
    expect(moduleFailureBody({ message: "Request failed" })).toBe("Request failed");
    expect(moduleFailureBody({ detail: "   ", message: "Network error" })).toBe("Network error");
    expect(moduleFailureBody({ detail: "", message: "  " })).toBe(MODULE_FAILURE_BODY);
    expect(moduleFailureBody(new Error("boom"))).toBe("boom");
  });

  it("reads a non-object refusal as the fallback", () => {
    expect(moduleFailureBody(null)).toBe(MODULE_FAILURE_BODY);
    expect(moduleFailureBody("nope")).toBe(MODULE_FAILURE_BODY);
    expect(moduleFailureBody({ detail: 42, message: null })).toBe(MODULE_FAILURE_BODY);
  });
});
