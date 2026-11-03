// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import { formatEstimatePoint, selectWorkItemContext, type WorkItemContextInput } from "./workItems.js";

const ISSUE_ID = "123e4567-e89b-12d3-a456-426614174000";
const ME_ID = "223e4567-e89b-12d3-a456-426614174001";
const STATE_ID = "323e4567-e89b-12d3-a456-426614174002";
const CYCLE_ID = "423e4567-e89b-12d3-a456-426614174003";
const MODULE_ID = "523e4567-e89b-12d3-a456-426614174004";
const LABEL_ID = "623e4567-e89b-12d3-a456-426614174005";

function input(overrides: Partial<WorkItemContextInput> = {}): WorkItemContextInput {
  return {
    workspaceSlug: "acme",
    projectId: "project-1",
    issueId: ISSUE_ID,
    issueKey: "ACME-12",
    stateId: null,
    priority: "medium",
    assigneeIds: [],
    labelIds: [],
    estimatePoint: null,
    cycleId: null,
    moduleIds: [],
    archived: false,
    estimatesEnabled: true,
    cycleView: true,
    moduleView: true,
    meId: ME_ID,
    subscribed: false,
    standing: { memberRole: 15, archived: false },
    mayDelete: true,
    pickers: {
      states: [{ id: STATE_ID, name: "Triage" } as never],
      labels: [{ id: LABEL_ID, name: "Bug" } as never],
      members: [{ userId: ME_ID, role: 15, displayName: "Ada" }],
      estimates: [
        {
          id: "e1",
          name: "Points",
          type: "points",
          last_used: true,
          points: [{ id: "p1", key: 1, value: "1" }],
        } as never,
      ],
      cycles: [{ id: CYCLE_ID, name: "Sprint 1" } as never],
      modules: [{ id: MODULE_ID, name: "Auth" } as never],
      loading: false,
    },
    actions: {
      updateFields: () => undefined,
      addToCycle: () => undefined,
      setModules: () => undefined,
      toggleSubscription: () => undefined,
      openDelete: () => undefined,
      copyId: () => undefined,
      copyTitle: () => undefined,
      copyUrl: () => undefined,
      close: () => undefined,
    },
    ...overrides,
  };
}

describe("selectWorkItemContext", () => {
  it("omits Delete for a viewer who is neither creator nor project admin", () => {
    const { commands } = selectWorkItemContext(input({ mayDelete: false }));
    expect(commands.map((command) => command.title)).not.toContain("Delete");
    expect(commands.map((command) => command.title)).toContain("Copy ID");
  });

  it("lists every editing entry plus the copy entries for editors", () => {
    const { commands, pages } = selectWorkItemContext(input());
    expect(commands.map((command) => command.title)).toEqual([
      "Change state",
      "Change priority",
      "Assign to",
      "Assign to me",
      "Change estimate",
      "Add to cycle",
      "Add to modules",
      "Add labels",
      "Subscribe to notifications",
      "Delete",
      "Copy ID",
      "Copy title",
      "Copy URL",
    ]);
    expect(new Set(commands.map((command) => command.section))).toEqual(new Set(["Work item actions"]));
    expect(commands.every((command) => command.context?.subject === "ACME-12")).toBe(true);
    expect(commands.every((command) => command.context?.indicator === "Work item")).toBe(true);
    expect(pages.map((page) => page.id)).toEqual([
      `context.work-item.${ISSUE_ID}.state`,
      `context.work-item.${ISSUE_ID}.priority`,
      `context.work-item.${ISSUE_ID}.assignee`,
      `context.work-item.${ISSUE_ID}.estimate`,
      `context.work-item.${ISSUE_ID}.cycle`,
      `context.work-item.${ISSUE_ID}.modules`,
      `context.work-item.${ISSUE_ID}.labels`,
    ]);
  });

  it("toggles the self-assign and subscription titles from state", () => {
    const assigned = selectWorkItemContext(input({ assigneeIds: [ME_ID], subscribed: true }));
    expect(assigned.commands.map((command) => command.title)).toContain("Un-assign from me");
    expect(assigned.commands.map((command) => command.title)).toContain("Unsubscribe from notifications");
  });

  it("hides editing entries for guests, outsiders and archived items", () => {
    for (const standing of [
      { memberRole: 5, archived: false },
      { memberRole: null, archived: false },
    ]) {
      const { commands, pages } = selectWorkItemContext(input({ standing }));
      expect(commands.map((command) => command.title)).toEqual(["Copy ID", "Copy title", "Copy URL"]);
      expect(pages).toEqual([]);
    }
    const archived = selectWorkItemContext(input({ archived: true }));
    expect(archived.commands.map((command) => command.title)).toEqual(["Copy ID", "Copy title", "Copy URL"]);
    const archivedProject = selectWorkItemContext(input({ standing: { memberRole: 20, archived: true } }));
    expect(archivedProject.commands.map((command) => command.title)).toEqual(["Copy ID", "Copy title", "Copy URL"]);
  });

  it("hides the estimate, cycle and module pickers with their surfaces", () => {
    const bare = selectWorkItemContext(input({ estimatesEnabled: false, cycleView: false, moduleView: false }));
    const titles = bare.commands.map((command) => command.title);
    expect(titles).not.toContain("Change estimate");
    expect(titles).not.toContain("Add to cycle");
    expect(titles).not.toContain("Add to modules");
    expect(titles).toContain("Change state");
  });

  it("declares the contextual shortcuts", () => {
    const { commands } = selectWorkItemContext(input());
    const byTitle = new Map(commands.map((command) => [command.title, command.shortcut]));
    expect(byTitle.get("Change state")).toBe("s");
    expect(byTitle.get("Change priority")).toBe("p");
    expect(byTitle.get("Assign to")).toBe("a");
    expect(byTitle.get("Assign to me")).toBe("i");
    expect(byTitle.get("Change estimate")).toBe("shift+e");
    expect(byTitle.get("Add to cycle")).toBe("shift+c");
    expect(byTitle.get("Add to modules")).toBe("shift+m");
    expect(byTitle.get("Add labels")).toBe("l");
    expect(byTitle.get("Subscribe to notifications")).toBe("shift+s");
    expect(byTitle.get("Delete")).toBe("mod+backspace");
    expect(byTitle.get("Copy ID")).toBe("mod+.");
    expect(byTitle.get("Copy title")).toBe("mod+shift+'");
    expect(byTitle.get("Copy URL")).toBe("mod+shift+,");
  });

  it("applies state, priority and assignee picks and no-ops unchanged picks", () => {
    const updateFields = vi.fn();
    const { pages } = selectWorkItemContext(input({ actions: { ...input().actions, updateFields } }));
    const state = pages.find((page) => page.placeholder === "Change state")!;
    expect(state.usePageState("").options).toHaveLength(1);
    state.usePageState("").options[0]!.run();
    expect(updateFields).toHaveBeenCalledWith({ state_id: STATE_ID });
    const priority = pages.find((page) => page.placeholder === "Change priority")!;
    priority
      .usePageState("")
      .options.find((option) => option.id === "medium")!
      .run();
    expect(updateFields).toHaveBeenCalledTimes(1);
    priority
      .usePageState("")
      .options.find((option) => option.id === "high")!
      .run();
    expect(updateFields).toHaveBeenCalledWith({ priority: "high" });
    const assignee = pages.find((page) => page.placeholder === "Assign to")!;
    assignee.usePageState("ada").options[0]!.run();
    expect(updateFields).toHaveBeenCalledWith({ assignee_ids: [ME_ID] });
    expect(assignee.usePageState("zzz").options).toHaveLength(0);
  });

  it("closes on single-change picks and stays open on multi-change picks", () => {
    const close = vi.fn();
    const { pages } = selectWorkItemContext(input({ actions: { ...input().actions, close } }));
    const runFirst = (placeholder: string): void => {
      pages
        .find((page) => page.placeholder === placeholder)!
        .usePageState("")
        .options[0]!.run();
    };
    for (const placeholder of ["Change state", "Change priority", "Change estimate", "Add to cycle"]) {
      close.mockClear();
      runFirst(placeholder);
      expect(close).toHaveBeenCalledTimes(1);
    }
    // Unchanged picks no-op the write but still confirm by closing.
    close.mockClear();
    pages
      .find((page) => page.placeholder === "Change priority")!
      .usePageState("")
      .options.find((option) => option.id === "medium")!
      .run();
    expect(close).toHaveBeenCalledTimes(1);
    for (const placeholder of ["Assign to", "Add labels", "Add to modules"]) {
      close.mockClear();
      runFirst(placeholder);
      expect(close).not.toHaveBeenCalled();
    }
  });

  it("toggles assignees, labels, modules and the self assignment", () => {
    const updateFields = vi.fn();
    const setModules = vi.fn();
    const base = input({
      assigneeIds: [ME_ID],
      labelIds: [LABEL_ID],
      moduleIds: [MODULE_ID],
      actions: { ...input().actions, updateFields, setModules },
    });
    const { commands, pages } = selectWorkItemContext(base);
    pages
      .find((page) => page.placeholder === "Assign to")!
      .usePageState("")
      .options[0]!.run();
    expect(updateFields).toHaveBeenCalledWith({ assignee_ids: [] });
    pages
      .find((page) => page.placeholder === "Add labels")!
      .usePageState("")
      .options[0]!.run();
    expect(updateFields).toHaveBeenCalledWith({ label_ids: [] });
    pages
      .find((page) => page.placeholder === "Add to modules")!
      .usePageState("")
      .options[0]!.run();
    expect(setModules).toHaveBeenCalledWith({ modules: [], removed_modules: [MODULE_ID] });
    commands.find((command) => command.title === "Un-assign from me")!.run();
    expect(updateFields).toHaveBeenCalledWith({ assignee_ids: [] });
  });

  it("adds modules by id list and assigns self when signed in", () => {
    const updateFields = vi.fn();
    const setModules = vi.fn();
    const { commands, pages } = selectWorkItemContext(
      input({ actions: { ...input().actions, updateFields, setModules } })
    );
    pages
      .find((page) => page.placeholder === "Add to modules")!
      .usePageState("")
      .options[0]!.run();
    expect(setModules).toHaveBeenCalledWith({ modules: [MODULE_ID] });
    commands.find((command) => command.title === "Assign to me")!.run();
    expect(updateFields).toHaveBeenCalledWith({ assignee_ids: [ME_ID] });
  });

  it("ignores self assignment without a signed-in user", () => {
    const updateFields = vi.fn();
    const { commands } = selectWorkItemContext(input({ meId: null, actions: { ...input().actions, updateFields } }));
    commands.find((command) => command.title === "Assign to me")!.run();
    expect(updateFields).not.toHaveBeenCalled();
  });

  it("applies estimate, cycle and subscription picks with no-ops", () => {
    const updateFields = vi.fn();
    const addToCycle = vi.fn();
    const toggleSubscription = vi.fn();
    const { commands, pages } = selectWorkItemContext(
      input({ actions: { ...input().actions, updateFields, addToCycle, toggleSubscription } })
    );
    const estimate = pages.find((page) => page.placeholder === "Change estimate")!;
    expect(estimate.usePageState("").options.map((option) => option.title)).toEqual(["No estimate", "1"]);
    estimate.usePageState("").options[0]!.run();
    expect(updateFields).not.toHaveBeenCalled();
    estimate.usePageState("").options[1]!.run();
    expect(updateFields).toHaveBeenCalledWith({ estimate_point: "p1" });
    const cycle = pages.find((page) => page.placeholder === "Add to cycle")!;
    cycle.usePageState("").options[0]!.run();
    expect(addToCycle).toHaveBeenCalledWith(CYCLE_ID);
    commands.find((command) => command.title === "Subscribe to notifications")!.run();
    expect(toggleSubscription).toHaveBeenCalledWith(false);
  });

  it("no-ops unchanged state and estimate-point picks but still closes", () => {
    const updateFields = vi.fn();
    const close = vi.fn();
    const { pages } = selectWorkItemContext(
      input({ stateId: STATE_ID, estimatePoint: "p1", actions: { ...input().actions, updateFields, close } })
    );
    const state = pages.find((page) => page.placeholder === "Change state")!;
    expect(state.usePageState("").options[0]!.selected).toBe(true);
    state.usePageState("").options[0]!.run();
    expect(updateFields).not.toHaveBeenCalled();
    expect(close).toHaveBeenCalledTimes(1);
    close.mockClear();
    const estimate = pages.find((page) => page.placeholder === "Change estimate")!;
    estimate
      .usePageState("")
      .options.find((option) => option.id === "p1")!
      .run();
    expect(updateFields).not.toHaveBeenCalled();
    expect(close).toHaveBeenCalledTimes(1);
  });

  it("lists the estimate page values in key order, not wire order", () => {
    const { pages } = selectWorkItemContext(
      input({
        pickers: {
          ...input().pickers,
          estimates: [
            {
              id: "e1",
              name: "Points",
              type: "points",
              last_used: true,
              points: [
                { id: "p2", key: 2, value: "3" },
                { id: "p1", key: 1, value: "1" },
              ],
            } as never,
          ],
        },
      })
    );
    const estimate = pages.find((page) => page.placeholder === "Change estimate")!;
    expect(estimate.usePageState("").options.map((option) => option.title)).toEqual(["No estimate", "1", "3"]);
  });

  it("filters the estimate page by the displayed time labels, not stored values", () => {
    // NEWFRONT-820: both surfaces search what the rows show. A stored
    // 90 reads "1h 30m", so "1h" narrows to it while "90" matches
    // nothing. The clear row matches its own label.
    const { pages } = selectWorkItemContext(
      input({
        pickers: {
          ...input().pickers,
          estimates: [
            {
              id: "e1",
              name: "Time",
              type: "time",
              last_used: true,
              points: [
                { id: "p90", key: 1, value: "90" },
                { id: "p45", key: 2, value: "45" },
              ],
            } as never,
          ],
        },
      })
    );
    const estimate = pages.find((page) => page.placeholder === "Change estimate")!;
    expect(estimate.usePageState("").options.map((option) => option.title)).toEqual(["No estimate", "1h 30m", "45m"]);
    expect(estimate.usePageState("1h").options.map((option) => option.title)).toEqual(["1h 30m"]);
    expect(estimate.usePageState("90").options).toEqual([]);
    expect(estimate.usePageState("no").options.map((option) => option.title)).toEqual(["No estimate"]);
  });

  it("offers only the clear row when no estimate system is active", () => {
    const updateFields = vi.fn();
    const close = vi.fn();
    const { pages } = selectWorkItemContext(
      input({
        estimatePoint: undefined,
        pickers: { ...input().pickers, estimates: [] },
        actions: { ...input().actions, updateFields, close },
      })
    );
    const estimate = pages.find((page) => page.placeholder === "Change estimate")!;
    expect(estimate.usePageState("").options.map((option) => option.title)).toEqual(["No estimate"]);
    estimate.usePageState("").options[0]!.run();
    expect(updateFields).not.toHaveBeenCalled();
    expect(close).toHaveBeenCalledTimes(1);
  });

  it("clears estimates and no-ops the current cycle", () => {
    const updateFields = vi.fn();
    const addToCycle = vi.fn();
    const { pages } = selectWorkItemContext(
      input({
        estimatePoint: "p1",
        cycleId: CYCLE_ID,
        actions: { ...input().actions, updateFields, addToCycle },
      })
    );
    pages
      .find((page) => page.placeholder === "Change estimate")!
      .usePageState("")
      .options[0]!.run();
    expect(updateFields).toHaveBeenCalledWith({ estimate_point: null });
    pages
      .find((page) => page.placeholder === "Add to cycle")!
      .usePageState("")
      .options[0]!.run();
    expect(addToCycle).not.toHaveBeenCalled();
  });

  it("runs the copy and delete actions", () => {
    const actions = {
      ...input().actions,
      openDelete: vi.fn(),
      copyId: vi.fn(),
      copyTitle: vi.fn(),
      copyUrl: vi.fn(),
    };
    const { commands } = selectWorkItemContext(input({ actions }));
    commands.find((command) => command.title === "Delete")!.run();
    commands.find((command) => command.title === "Copy ID")!.run();
    commands.find((command) => command.title === "Copy title")!.run();
    commands.find((command) => command.title === "Copy URL")!.run();
    expect(actions.openDelete).toHaveBeenCalledTimes(1);
    expect(actions.copyId).toHaveBeenCalledTimes(1);
    expect(actions.copyTitle).toHaveBeenCalledTimes(1);
    expect(actions.copyUrl).toHaveBeenCalledTimes(1);
  });

  it("spins the pickers while their lists load", () => {
    const { pages } = selectWorkItemContext(input({ pickers: { ...input().pickers, loading: true } }));
    for (const page of pages) {
      if (page.placeholder === "Change priority") continue;
      expect(page.usePageState("").status).toBe("loading");
    }
  });
});

describe("formatEstimatePoint", () => {
  it("passes points and categories through and formats time systems", () => {
    expect(formatEstimatePoint("3", "points")).toBe("3");
    expect(formatEstimatePoint("M", "categories")).toBe("M");
    expect(formatEstimatePoint("45", "time")).toBe("45m");
    expect(formatEstimatePoint("120", "time")).toBe("2h");
    expect(formatEstimatePoint("90", "time")).toBe("1h 30m");
    expect(formatEstimatePoint("soon", "time")).toBe("soon");
    expect(formatEstimatePoint("-30", "time")).toBe("-30");
  });
});
