// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  buildCreatePayload,
  buildDraftInput,
  canGenerateTitle,
  descriptionText,
  emptyCreateForm,
  hasDraftWorthKeeping,
  hasMembership,
  isCreatableProjectId,
  isTitleAcceptable,
  membershipForForm,
  normalizeTitle,
  resetForCreateMore,
  resolveProjectId,
  selectCreatableProjects,
  selectCreatableWorkItemProjects,
  targetProjectId,
  titleError,
  toCreateProjectOption,
  TITLE_MAX_LENGTH,
  UNTITLED_DRAFT_NAME,
  type CreateFormState,
  type CreateProjectOption,
} from "./createForm.js";

const PROJECT = "11111111-1111-4111-8111-111111111111";
const OTHER = "22222222-2222-4222-8222-222222222222";

function project(overrides: Partial<CreateProjectOption> = {}): CreateProjectOption {
  return { id: PROJECT, name: "Atlas", identifier: "ATL", memberRole: 15, archived: false, ...overrides };
}

function form(overrides: Partial<CreateFormState> = {}): CreateFormState {
  return { ...emptyCreateForm(PROJECT), ...overrides };
}

describe("selectCreatableProjects", () => {
  it("keeps member and admin projects, drops guests, archived rows and non-members", () => {
    const rows = [
      project({ id: "member", memberRole: 15 }),
      project({ id: "admin", memberRole: 20 }),
      project({ id: "guest", memberRole: 5 }),
      project({ id: "outsider", memberRole: null }),
      project({ id: "archived", memberRole: 15, archived: true }),
    ];
    expect(selectCreatableProjects(rows).map((row) => row.id)).toEqual(["member", "admin"]);
  });

  it("reads an empty list when nothing is creatable", () => {
    expect(selectCreatableProjects([project({ memberRole: 5 })])).toEqual([]);
  });
});

describe("resolveProjectId", () => {
  const rows = [project(), project({ id: OTHER, name: "Beacon" })];

  it("keeps a preferred project that is still offered", () => {
    expect(resolveProjectId(rows, OTHER)).toBe(OTHER);
  });

  it("falls back to the first offered project", () => {
    expect(resolveProjectId(rows, "gone")).toBe(PROJECT);
  });

  it("returns null without projects", () => {
    expect(resolveProjectId([], OTHER)).toBeNull();
  });
});

describe("title rules", () => {
  it("trims before judging a title", () => {
    expect(normalizeTitle("  Fix login  ")).toBe("Fix login");
    expect(isTitleAcceptable("   ")).toBe(false);
    expect(isTitleAcceptable("  Fix login ")).toBe(true);
  });

  it("caps the title at 255 including whitespace", () => {
    expect(TITLE_MAX_LENGTH).toBe(255);
    expect(isTitleAcceptable("n".repeat(255))).toBe(true);
    expect(isTitleAcceptable("n".repeat(256))).toBe(false);
  });

  it("explains each rejection", () => {
    expect(titleError("")).toBe("Give the work item a title.");
    expect(titleError("   ")).toBe("Give the work item a title.");
    expect(titleError("n".repeat(256))).toContain("255");
    expect(titleError("Fix login")).toBeNull();
  });
});

describe("canGenerateTitle", () => {
  it("needs a configured assistant, a blank title and a non-empty description", () => {
    expect(canGenerateTitle({ name: "  ", descriptionHtml: "<p>Words</p>", assistantConfigured: true })).toBe(true);
  });

  it("refuses without a configured assistant", () => {
    expect(canGenerateTitle({ name: "", descriptionHtml: "<p>Words</p>", assistantConfigured: false })).toBe(false);
  });

  it("refuses when a title exists", () => {
    expect(canGenerateTitle({ name: "Named", descriptionHtml: "<p>Words</p>", assistantConfigured: true })).toBe(false);
  });

  it("refuses without description text", () => {
    expect(canGenerateTitle({ name: "", descriptionHtml: "<p> </p>", assistantConfigured: true })).toBe(false);
    expect(canGenerateTitle({ name: "", descriptionHtml: "", assistantConfigured: true })).toBe(false);
  });

  it("reads description text without markup", () => {
    expect(descriptionText("<p>Hello <strong>world</strong></p>")).toBe("Hello world");
  });
});

describe("buildCreatePayload", () => {
  it("sends only the title for a bare form", () => {
    expect(buildCreatePayload(form({ name: "  Fresh report  ", descriptionHtml: "" }))).toEqual({
      name: "Fresh report",
    });
  });

  it("carries every field the user set", () => {
    const payload = buildCreatePayload(
      form({
        name: "Full",
        descriptionHtml: "<p>Body</p>",
        stateId: "state",
        priority: "urgent",
        assigneeIds: ["member"],
        labelIds: ["label"],
        startDate: "2026-01-02",
        targetDate: "2026-01-03",
        estimatePoint: "point",
        parentId: "parent",
        execution: { type: "pod", podId: "pod" },
      })
    );
    expect(payload).toEqual({
      name: "Full",
      description_html: "<p>Body</p>",
      state_id: "state",
      priority: "urgent",
      assignee_ids: ["member"],
      label_ids: ["label"],
      start_date: "2026-01-02",
      target_date: "2026-01-03",
      estimate_point: "point",
      parent_id: "parent",
      assigned_pod_id: "pod",
      agent_executor: "local_runner",
    });
  });

  it("pins an agent executor with no pod", () => {
    const payload = buildCreatePayload(form({ name: "Agent", execution: { type: "agent", executor: "cloud_agent" } }));
    expect(payload.assigned_pod_id).toBeNull();
    expect(payload.agent_executor).toBe("cloud_agent");
  });

  it("leaves the priority out when it is none", () => {
    expect(buildCreatePayload(form({ name: "Plain", priority: "none" }))).toEqual({ name: "Plain" });
  });

  it("never puts cycle or module on the create body; they attach after", () => {
    const payload = buildCreatePayload(form({ name: "Scheduled", cycleId: "cycle", moduleIds: ["module"] }));
    expect(payload).not.toHaveProperty("cycle_id");
    expect(payload).not.toHaveProperty("module_ids");
  });
});

describe("create membership", () => {
  it("copies the cycle and modules the form holds", () => {
    const source = form({ cycleId: "cycle", moduleIds: ["a", "b"] });
    const membership = membershipForForm(source);
    expect(membership).toEqual({ cycleId: "cycle", moduleIds: ["a", "b"] });
    membership.moduleIds.push("c");
    expect(source.moduleIds).toEqual(["a", "b"]);
  });

  it("is empty for a plain form", () => {
    expect(hasMembership(membershipForForm(form()))).toBe(false);
    expect(hasMembership({ cycleId: "cycle", moduleIds: [] })).toBe(true);
    expect(hasMembership({ cycleId: null, moduleIds: ["module"] })).toBe(true);
  });
});

describe("buildDraftInput", () => {
  it("falls back to an untitled draft and keeps the description", () => {
    const draft = buildDraftInput(form({ name: "   ", descriptionHtml: "<p>Half-written</p>" }));
    expect(draft.name).toBe(UNTITLED_DRAFT_NAME);
    expect(draft.description_html).toBe("<p>Half-written</p>");
  });

  it("trims a real title and carries the set fields", () => {
    const draft = buildDraftInput(
      form({
        name: " Draft me ",
        priority: "high",
        assigneeIds: ["member"],
        labelIds: ["label"],
        startDate: "2026-02-01",
        targetDate: "2026-02-02",
        estimatePoint: "point",
        parentId: "parent",
      })
    );
    expect(draft).toEqual({
      name: "Draft me",
      project_id: PROJECT,
      description_html: "",
      priority: "high",
      assignee_ids: ["member"],
      label_ids: ["label"],
      start_date: "2026-02-01",
      target_date: "2026-02-02",
      parent_id: "parent",
    });
  });

  it("sends the state and omits a none priority", () => {
    const draft = buildDraftInput(form({ stateId: "state", priority: "none" }));
    expect(draft).toEqual({ name: UNTITLED_DRAFT_NAME, project_id: PROJECT, description_html: "", state_id: "state" });
  });
});

describe("hasDraftWorthKeeping", () => {
  it("ignores the project and an empty description", () => {
    expect(hasDraftWorthKeeping(form({ projectId: PROJECT, descriptionHtml: "<p> </p>" }))).toBe(false);
  });

  const dirty: Array<[string, Partial<CreateFormState>]> = [
    ["title", { name: "Typed" }],
    ["description", { descriptionHtml: "<p>Words</p>" }],
    ["state", { stateId: "state" }],
    ["priority", { priority: "low" }],
    ["assignees", { assigneeIds: ["member"] }],
    ["labels", { labelIds: ["label"] }],
    ["start date", { startDate: "2026-01-02" }],
    ["due date", { targetDate: "2026-01-02" }],
    ["cycle", { cycleId: "cycle" }],
    ["modules", { moduleIds: ["module"] }],
    ["estimate", { estimatePoint: "point" }],
    ["execution target", { execution: { type: "pod", podId: "pod" } }],
    ["parent", { parentId: "parent" }],
  ];

  it.each(dirty)("counts %s", (_label, override) => {
    expect(hasDraftWorthKeeping(form(override))).toBe(true);
  });
});

describe("selectCreatableWorkItemProjects", () => {
  it("fills a caller's rows and keeps only member-or-admin projects", () => {
    const rows = [
      { id: "p1", name: "Atlas", memberRole: 15 },
      { id: "p2", name: "Guestland", memberRole: 5 },
      { id: "p3", name: "Old", memberRole: 20, archived: true },
      { id: "p4", name: "Bare" },
    ];
    expect(selectCreatableWorkItemProjects(rows).map((row) => row.id)).toEqual(["p1"]);
    expect(toCreateProjectOption(rows[3]!)).toMatchObject({ identifier: "BAR", memberRole: null, archived: false });
  });
});

describe("isCreatableProjectId", () => {
  it("accepts only a project from the create list", () => {
    const available = [project({ id: "p1" })];
    expect(isCreatableProjectId("p1", available)).toBe(true);
    expect(isCreatableProjectId("guest", available)).toBe(false);
    expect(isCreatableProjectId("p1", [])).toBe(false);
  });
});

describe("targetProjectId", () => {
  it("keeps the form's project when it has one", () => {
    expect(targetProjectId("p2", [project({ id: "p1" })])).toBe("p2");
  });

  it("falls back to the first creatable project", () => {
    expect(targetProjectId(null, [project({ id: "p1" })])).toBe("p1");
  });

  it("answers an empty string when nothing is offered", () => {
    expect(targetProjectId(null, [])).toBe("");
  });
});

describe("resetForCreateMore", () => {
  it("keeps the project and the description default and clears the rest", () => {
    const filled = form({
      name: "First",
      descriptionHtml: "<p>typed</p>",
      priority: "urgent",
      labelIds: ["label"],
      parentId: "parent",
      execution: { type: "pod", podId: "pod" },
    });
    expect(resetForCreateMore(filled, "<p>default</p>")).toEqual({
      ...emptyCreateForm(PROJECT, "<p>default</p>"),
    });
  });

  it("keeps a null project", () => {
    expect(resetForCreateMore(form({ projectId: null, name: "x" })).projectId).toBeNull();
  });
});
