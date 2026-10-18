// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  states: vi.fn(),
  members: vi.fn(),
  directory: vi.fn(),
  projects: vi.fn(),
  cycles: vi.fn(),
  modules: vi.fn(),
  labels: vi.fn(),
  me: vi.fn(),
}));

vi.mock("../../../shared/pickers/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/pickers/index.js")>()),
  usePickerStates: mocks.states,
  usePickerProjectMembers: mocks.members,
  usePickerWorkspaceMembers: mocks.directory,
  usePickerProjects: mocks.projects,
  usePickerCycles: mocks.cycles,
  usePickerModules: mocks.modules,
}));

vi.mock("../labels/api.js", () => ({ useLabels: mocks.labels }));
vi.mock("../../../core/session/index.js", () => ({ useMe: mocks.me }));

import { useListReferences } from "./reference.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ME = "323e4567-e89b-12d3-a456-426614174002";
const OTHER = "423e4567-e89b-12d3-a456-426614174003";

function resolved<T>(data: T) {
  return { data, isPending: false };
}

function state(id: string, name: string) {
  return {
    id,
    project_id: PROJECT,
    workspace_id: "",
    name,
    color: "#fff",
    group: "unstarted",
    default: false,
    description: "",
    sequence: 1,
  };
}

function directoryRow(id: string, name: string) {
  return {
    id: `${id}-membership`,
    member: { id, first_name: name, last_name: "", avatar: "", avatar_url: null, display_name: name, is_bot: false },
    role: 15,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.states.mockReturnValue(resolved([state(ME, "Todo"), state(OTHER, "Done")]));
  mocks.members.mockReturnValue(
    resolved([{ id: "m1", role: 15, member: ME, project: PROJECT, original_role: 15, created_at: "" }])
  );
  mocks.directory.mockReturnValue(resolved([directoryRow(ME, "Ana"), directoryRow(OTHER, "Zoe")]));
  mocks.projects.mockReturnValue(resolved([{ id: PROJECT, name: "Website" }]));
  mocks.cycles.mockReturnValue(
    resolved([
      { id: "cycle-open", name: "Sprint 1", status: "current" },
      { id: "cycle-done", name: "Sprint 0", status: "completed" },
    ])
  );
  mocks.modules.mockReturnValue(resolved([{ id: "module-1", name: "Auth" }]));
  mocks.labels.mockReturnValue(resolved([{ id: "label-1", name: "bug" }]));
  mocks.me.mockReturnValue(resolved({ id: ME }));
});

describe("useListReferences", () => {
  it("builds state, label, cycle, module and project name maps", () => {
    const { result } = renderHook(() =>
      useListReferences({ workspaceSlug: "acme", projectId: PROJECT, dimension: null })
    );
    expect(result.current.stateRows).toHaveLength(2);
    expect(result.current.stateNames.get(ME)).toBe("Todo");
    expect(result.current.labelNames.get("label-1")).toBe("bug");
    expect(result.current.cycleNames.get("cycle-open")).toBe("Sprint 1");
    expect(result.current.moduleNames.get("module-1")).toBe("Auth");
    expect(result.current.projectNames.get(PROJECT)).toBe("Website");
    expect(result.current.labels).toMatchObject({
      state: result.current.stateNames,
      labels: result.current.labelNames,
      cycle: result.current.cycleNames,
      module: result.current.moduleNames,
      project: result.current.projectNames,
    });
  });

  it("keeps only open cycles in the read-only set", () => {
    const { result } = renderHook(() =>
      useListReferences({ workspaceSlug: "acme", projectId: PROJECT, dimension: "cycle" })
    );
    expect(result.current.openCycleIds.has("cycle-open")).toBe(true);
    expect(result.current.openCycleIds.has("cycle-done")).toBe(false);
  });

  it("joins assignee names from project members and the directory", () => {
    const { result } = renderHook(() =>
      useListReferences({ workspaceSlug: "acme", projectId: PROJECT, dimension: "assignees" })
    );
    expect(result.current.assigneeNames.get(ME)).toBe("Ana");
  });

  it("adds creator labels from the workspace directory when grouping by creator", () => {
    const { result } = renderHook(() =>
      useListReferences({ workspaceSlug: "acme", projectId: PROJECT, dimension: "created_by" })
    );
    expect(result.current.labels.created_by?.get(OTHER)).toBe("Zoe");
  });

  it("omits creator labels for other dimensions", () => {
    const { result } = renderHook(() =>
      useListReferences({ workspaceSlug: "acme", projectId: PROJECT, dimension: "state" })
    );
    expect(result.current.labels.created_by).toBeUndefined();
  });

  it("reports loading from states, projects and labels regardless of dimension", () => {
    mocks.states.mockReturnValue({ data: undefined, isPending: true });
    const { result } = renderHook(() =>
      useListReferences({ workspaceSlug: "acme", projectId: PROJECT, dimension: "state" })
    );
    expect(result.current.isLoading).toBe(true);
    expect(result.current.stateRows).toEqual([]);
  });

  it("ignores assignee, cycle and module loads the dimension does not need", () => {
    mocks.members.mockReturnValue({ data: undefined, isPending: true });
    mocks.directory.mockReturnValue({ data: undefined, isPending: true });
    mocks.cycles.mockReturnValue({ data: undefined, isPending: true });
    mocks.modules.mockReturnValue({ data: undefined, isPending: true });
    const { result } = renderHook(() =>
      useListReferences({ workspaceSlug: "acme", projectId: PROJECT, dimension: "state" })
    );
    expect(result.current.isLoading).toBe(false);
  });

  it("counts a needed dimension's pending read as loading", () => {
    mocks.cycles.mockReturnValue({ data: undefined, isPending: true });
    const { result } = renderHook(() =>
      useListReferences({ workspaceSlug: "acme", projectId: PROJECT, dimension: "cycle" })
    );
    expect(result.current.isLoading).toBe(true);
  });
});
