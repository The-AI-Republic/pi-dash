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
  initialCreateForm,
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

  it("carries the picked work-item type and omits the default (ISS-129)", () => {
    expect(buildCreatePayload(form({ name: "Typed" }), "t-bug")).toEqual({
      name: "Typed",
      type: "t-bug",
    });
    expect(buildCreatePayload(form({ name: "Typed" }), null)).toEqual({ name: "Typed" });
    expect(buildCreatePayload(form({ name: "Typed" }))).toEqual({ name: "Typed" });
  });

  it("drops the editor's mount-time empty paragraph", () => {
    expect(buildCreatePayload(form({ name: "Fresh", descriptionHtml: "<p></p>" }))).toEqual({ name: "Fresh" });
  });

  it("keeps a description made only of an image or a mention (ISS-127)", () => {
    const image = '<p><img src="https://s3.test/a.png"></p>';
    const mention = '<p><mention-component id="u-1"></mention-component></p>';
    expect(buildCreatePayload(form({ name: "Shot", descriptionHtml: image }))).toEqual({
      name: "Shot",
      description_html: image,
    });
    expect(buildCreatePayload(form({ name: "Ping", descriptionHtml: mention }))).toEqual({
      name: "Ping",
      description_html: mention,
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
      assigned_runner_id: null,
      agent_executor: "local_runner",
    });
  });

  it("pins an agent executor with no pod", () => {
    const payload = buildCreatePayload(form({ name: "Agent", execution: { type: "agent", executor: "cloud_agent" } }));
    expect(payload.assigned_pod_id).toBeNull();
    expect(payload.assigned_runner_id).toBeNull();
    expect(payload.agent_executor).toBe("cloud_agent");
  });

  it("pins a runner inside its pod", () => {
    const payload = buildCreatePayload(
      form({ name: "Runner", execution: { type: "runner", runnerId: "runner-1", podId: "pod" } })
    );
    expect(payload.assigned_pod_id).toBe("pod");
    expect(payload.assigned_runner_id).toBe("runner-1");
    expect(payload.agent_executor).toBe("local_runner");
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

  it("keeps an image-only body in the draft", () => {
    const image = '<p><img src="https://s3.test/a.png"></p>';
    expect(buildDraftInput(form({ name: "", descriptionHtml: image })).description_html).toBe(image);
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
      estimate_point: "point",
      parent_id: "parent",
    });
  });

  it("keeps a draft's cycle, modules and estimate on the row", () => {
    const draft = buildDraftInput(
      form({ cycleId: "cycle-1", moduleIds: ["module-1", "module-2"], estimatePoint: "point" })
    );
    expect(draft.cycle_id).toBe("cycle-1");
    expect(draft.module_ids).toEqual(["module-1", "module-2"]);
    expect(draft.estimate_point).toBe("point");
  });

  it("sends the state and omits a none priority", () => {
    const draft = buildDraftInput(form({ stateId: "state", priority: "none" }));
    expect(draft).toEqual({ name: UNTITLED_DRAFT_NAME, project_id: PROJECT, description_html: "", state_id: "state" });
  });
});

describe("hasDraftWorthKeeping", () => {
  it("ignores the project and an empty description", () => {
    expect(hasDraftWorthKeeping(form({ projectId: PROJECT, descriptionHtml: "<p> </p>" }))).toBe(false);
    expect(hasDraftWorthKeeping(form({ projectId: PROJECT, descriptionHtml: "<p></p>" }))).toBe(false);
  });

  it("does not count membership a route seeded, but counts one the user picked", () => {
    const engaged = { cycleId: "cycle", moduleIds: ["module"] };
    expect(hasDraftWorthKeeping(form(engaged), engaged)).toBe(false);
    expect(hasDraftWorthKeeping(form({ cycleId: "other" }), engaged)).toBe(true);
    expect(hasDraftWorthKeeping(form({ moduleIds: ["other"] }), engaged)).toBe(true);
    expect(hasDraftWorthKeeping(form({ cycleId: "cycle" }))).toBe(true);
    expect(hasDraftWorthKeeping(form({ moduleIds: ["module"] }))).toBe(true);
    // Same size, different member: the user swapped one module for another.
    expect(hasDraftWorthKeeping(form({ moduleIds: ["other"] }), engaged)).toBe(true);
    expect(hasDraftWorthKeeping(form({ moduleIds: [] }), { cycleId: null, moduleIds: ["a"] })).toBe(true);
  });

  it("counts an image-only or mention-only description", () => {
    expect(hasDraftWorthKeeping(form({ descriptionHtml: '<p><img src="x"></p>' }))).toBe(true);
    expect(
      hasDraftWorthKeeping(form({ descriptionHtml: '<p><mention-component id="u-1"></mention-component></p>' }))
    ).toBe(true);
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

  it("keeps the route's cycle and modules so a second item lands there too", () => {
    const next = resetForCreateMore(form({ name: "First", cycleId: "cycle", moduleIds: ["module"] }));
    expect(next.cycleId).toBe("cycle");
    expect(next.moduleIds).toEqual(["module"]);
    expect(next.name).toBe("");
  });
});

describe("initialCreateForm", () => {
  it("applies a section's grouping value over a blank form", () => {
    const form = initialCreateForm(PROJECT, {
      stateId: "state-1",
      priority: "high",
      assigneeIds: ["member-1"],
      labelIds: ["label-1"],
      cycleId: "cycle-1",
      moduleIds: ["module-1"],
      targetDate: "2026-12-01",
    });
    expect(form).toMatchObject({
      projectId: PROJECT,
      stateId: "state-1",
      priority: "high",
      assigneeIds: ["member-1"],
      labelIds: ["label-1"],
      cycleId: "cycle-1",
      moduleIds: ["module-1"],
      targetDate: "2026-12-01",
    });
  });

  it("leaves the form blank when the section has no value", () => {
    expect(initialCreateForm(PROJECT)).toEqual(emptyCreateForm(PROJECT));
  });
});
