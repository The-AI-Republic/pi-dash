// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members write hooks (parity/specs/project-settings-members.md §"Add
// members", §"Inline role changes", §"Remove and leave-self", §"Project
// defaults"; PSET-022–026): each hook calls its contract with the route's
// ids, folds the answer into the caches, and toasts refusals. The project
// defaults read and the editor identity come from the shared auth reads.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { ApiError } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: {
    addProjectMembers: vi.fn(),
    updateProjectMember: vi.fn(),
    removeProjectMember: vi.fn(),
    leaveProject: vi.fn(),
    updateProject: vi.fn(),
    listProjectDetails: vi.fn(),
  },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

vi.mock("../../../shared/shell/index.js", () => ({ notifyError: vi.fn() }));

const authMocks = vi.hoisted(() => ({
  me: { data: { id: "123e4567-e89b-12d3-a456-42661417400a" }, isPending: false },
  workspace: { data: { role: 15 }, isPending: false },
  roles: { data: { "223e4567-e89b-12d3-a456-426614174001": 20 } as Record<string, number>, isPending: false },
  project: { queryKey: ["ws", "acme", "auth", "project", "p"] as const },
}));

vi.mock("../../auth/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  authKeys: {
    project: () => authMocks.project.queryKey,
    projectRoles: () => ["ws", "acme", "auth", "project-roles"],
  },
  projectQueryOptions: () => ({
    queryKey: authMocks.project.queryKey,
    queryFn: async () => contractMocks.getProject(),
  }),
  useWorkspaceMembership: () => authMocks.workspace,
  useProjectRoles: () => authMocks.roles,
}));

vi.mock("../../projects/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  projectKeys: { details: () => ["ws", "acme", "projects", "details"] },
  projectDetailsQueryOptions: () => ({
    queryKey: ["ws", "acme", "projects", "details"],
    queryFn: async () => contractMocks.listProjectDetails(),
  }),
}));

vi.mock("../../../core/session/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useMe: () => authMocks.me,
}));

import { notifyError } from "../../../shared/shell/index.js";
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  INVITE_MEMBERS_FAILED_TITLE,
  LEAVE_PROJECT_FAILED_TITLE,
  memberKeys,
  REMOVE_MEMBER_FAILED_TITLE,
  SAVE_DEFAULTS_FAILED_TITLE,
  UPDATE_ROLE_FAILED_TITLE,
  useAddProjectMembers,
  useLeaveProject,
  useMemberEditor,
  useProjectDefaults,
  useRemoveProjectMember,
  useSaveProjectDefaults,
  useUpdateProjectMemberRole,
  WRITE_FAILED_FALLBACK,
} from "./api.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MEMBERSHIP = "323e4567-e89b-12d3-a456-426614174001";

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

function newClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
}

const addedRow = { id: MEMBERSHIP, role: 15, member: "u", project: PROJECT, original_role: 15, created_at: "x" };
const detailRow = { id: MEMBERSHIP, role: 15 };
const projectRow = { id: PROJECT, project_lead: "u", default_assignee: null, guest_view_all_features: true };

beforeEach(() => {
  createAppClient("https://api.test");
});

afterEach(() => {
  resetAppClient();
  for (const fn of Object.values(contractMocks)) fn.mockReset();
  vi.mocked(notifyError).mockReset();
});

describe("useAddProjectMembers", () => {
  it("bulk-adds the rows and refreshes the memberships and directory", async () => {
    contractMocks.addProjectMembers.mockResolvedValue([addedRow]);
    const queryClient = newClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useAddProjectMembers(SLUG, PROJECT), { wrapper: wrapper(queryClient) });
    await act(async () => {
      result.current.mutate({ members: [{ member_id: "u", role: 15 }] });
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(contractMocks.addProjectMembers).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, [
      { member_id: "u", role: 15 },
    ]);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: memberKeys.memberships(SLUG, PROJECT) });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: memberKeys.directory(SLUG) });
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("toasts the server refusal", async () => {
    contractMocks.addProjectMembers.mockRejectedValue(new ApiError({ status: 400, code: "http", message: "Nope." }));
    const { result } = renderHook(() => useAddProjectMembers(SLUG, PROJECT), { wrapper: wrapper(newClient()) });
    await act(async () => {
      result.current.mutate({ members: [] });
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith(INVITE_MEMBERS_FAILED_TITLE, "Nope.");
  });
});

describe("useUpdateProjectMemberRole", () => {
  it("patches the membership role and refreshes the roster", async () => {
    contractMocks.updateProjectMember.mockResolvedValue(detailRow);
    const queryClient = newClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const rollback = vi.fn();
    const { result } = renderHook(() => useUpdateProjectMemberRole(SLUG, PROJECT, rollback), {
      wrapper: wrapper(queryClient),
    });
    await act(async () => {
      result.current.mutate({ memberId: MEMBERSHIP, role: 20 });
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(contractMocks.updateProjectMember).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, MEMBERSHIP, 20);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: memberKeys.memberships(SLUG, PROJECT) });
    expect(rollback).not.toHaveBeenCalled();
  });

  it("rolls the optimistic value back and toasts the server message", async () => {
    contractMocks.updateProjectMember.mockRejectedValue(
      new ApiError({ status: 403, code: "http", message: "You cannot assign a role equal to or higher than your own" })
    );
    const rollback = vi.fn();
    const { result } = renderHook(() => useUpdateProjectMemberRole(SLUG, PROJECT, rollback), {
      wrapper: wrapper(newClient()),
    });
    await act(async () => {
      result.current.mutate({ memberId: MEMBERSHIP, role: 20 });
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(rollback).toHaveBeenCalledWith({ memberId: MEMBERSHIP, role: 20 });
    expect(notifyError).toHaveBeenCalledWith(
      UPDATE_ROLE_FAILED_TITLE,
      "You cannot assign a role equal to or higher than your own"
    );
  });

  it("works without a rollback callback and falls back when the error has no text", async () => {
    contractMocks.updateProjectMember.mockRejectedValue(new Error("socket closed"));
    const { result } = renderHook(() => useUpdateProjectMemberRole(SLUG, PROJECT), { wrapper: wrapper(newClient()) });
    await act(async () => {
      result.current.mutate({ memberId: MEMBERSHIP, role: 5 });
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith(UPDATE_ROLE_FAILED_TITLE, WRITE_FAILED_FALLBACK);
  });
});

describe("useRemoveProjectMember", () => {
  it("deletes the membership and refreshes the roster", async () => {
    contractMocks.removeProjectMember.mockResolvedValue(204);
    const queryClient = newClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useRemoveProjectMember(SLUG, PROJECT), { wrapper: wrapper(queryClient) });
    await act(async () => {
      result.current.mutate({ memberId: MEMBERSHIP });
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(contractMocks.removeProjectMember).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, MEMBERSHIP);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: memberKeys.memberships(SLUG, PROJECT) });
  });

  it("toasts a refused removal", async () => {
    contractMocks.removeProjectMember.mockRejectedValue(
      new ApiError({ status: 404, code: "http", message: "Membership is gone." })
    );
    const { result } = renderHook(() => useRemoveProjectMember(SLUG, PROJECT), { wrapper: wrapper(newClient()) });
    await act(async () => {
      result.current.mutate({ memberId: MEMBERSHIP });
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith(REMOVE_MEMBER_FAILED_TITLE, "Membership is gone.");
  });
});

describe("useLeaveProject", () => {
  it("calls the leave endpoint and drops the stale feeds", async () => {
    contractMocks.leaveProject.mockResolvedValue(204);
    const queryClient = newClient();
    const remove = vi.spyOn(queryClient, "removeQueries");
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useLeaveProject(SLUG, PROJECT), { wrapper: wrapper(queryClient) });
    await act(async () => {
      result.current.mutate();
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(contractMocks.leaveProject).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT);
    expect(remove).toHaveBeenCalledWith({ queryKey: authMocks.project.queryKey });
    expect(remove).toHaveBeenCalledWith({ queryKey: memberKeys.memberships(SLUG, PROJECT) });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["ws", "acme", "auth", "project-roles"] });
  });

  it("toasts the refuse message and keeps the membership", async () => {
    contractMocks.leaveProject.mockRejectedValue(
      new ApiError({ status: 400, code: "http", message: "You cannot leave the project as your the only admin" })
    );
    const { result } = renderHook(() => useLeaveProject(SLUG, PROJECT), { wrapper: wrapper(newClient()) });
    await act(async () => {
      result.current.mutate();
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith(
      LEAVE_PROJECT_FAILED_TITLE,
      "You cannot leave the project as your the only admin"
    );
  });
});

describe("useSaveProjectDefaults", () => {
  it("saves one field and folds the answered row into both project caches", async () => {
    contractMocks.updateProject.mockResolvedValue(projectRow);
    const queryClient = newClient();
    queryClient.setQueryData(authMocks.project.queryKey, { id: PROJECT, name: "Keep me" });
    queryClient.setQueryData(
      ["ws", "acme", "projects", "details"],
      [
        { id: PROJECT, name: "Old" },
        { id: "other", name: "Other" },
      ]
    );
    const { result } = renderHook(() => useSaveProjectDefaults(SLUG, PROJECT), { wrapper: wrapper(queryClient) });
    await act(async () => {
      result.current.mutate({ project_lead: "u" });
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(contractMocks.updateProject).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, { project_lead: "u" });
    expect(queryClient.getQueryData(authMocks.project.queryKey)).toMatchObject({
      id: PROJECT,
      name: "Keep me",
      project_lead: "u",
    });
    expect(queryClient.getQueryData(["ws", "acme", "projects", "details"])).toEqual([
      expect.objectContaining({ id: PROJECT, project_lead: "u" }),
      expect.objectContaining({ id: "other", name: "Other" }),
    ]);
  });

  it("leaves absent caches absent", async () => {
    contractMocks.updateProject.mockResolvedValue(projectRow);
    const queryClient = newClient();
    const { result } = renderHook(() => useSaveProjectDefaults(SLUG, PROJECT), { wrapper: wrapper(queryClient) });
    await act(async () => {
      result.current.mutate({ guest_view_all_features: true });
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(queryClient.getQueryData(authMocks.project.queryKey)).toBeUndefined();
    expect(queryClient.getQueryData(["ws", "acme", "projects", "details"])).toBeUndefined();
  });

  it("toasts a refused save", async () => {
    contractMocks.updateProject.mockRejectedValue(new ApiError({ status: 403, code: "http", message: "Admins only." }));
    const { result } = renderHook(() => useSaveProjectDefaults(SLUG, PROJECT), { wrapper: wrapper(newClient()) });
    await act(async () => {
      result.current.mutate({ default_assignee: null });
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith(SAVE_DEFAULTS_FAILED_TITLE, "Admins only.");
  });
});

describe("useProjectDefaults and useMemberEditor", () => {
  it("picks this project's defaults out of the workspace detail rows", async () => {
    contractMocks.listProjectDetails.mockResolvedValue([
      {
        ...projectRow,
        id: "other-project",
        project_lead: null,
        default_assignee: null,
        guest_view_all_features: false,
      },
      projectRow,
    ]);
    const { result } = renderHook(() => useProjectDefaults(SLUG, PROJECT), { wrapper: wrapper(newClient()) });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual(projectRow);
  });

  it("answers undefined while the detail rows are pending or miss the project", async () => {
    contractMocks.listProjectDetails.mockResolvedValue([{ ...projectRow, id: "other-project" }]);
    const { result } = renderHook(() => useProjectDefaults(SLUG, PROJECT), { wrapper: wrapper(newClient()) });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toBeUndefined();
  });

  it("resolves the editor's id, workspace role and project role", () => {
    const { result } = renderHook(() => useMemberEditor(SLUG, PROJECT), { wrapper: wrapper(newClient()) });
    expect(result.current).toEqual({
      userId: "123e4567-e89b-12d3-a456-42661417400a",
      workspaceRole: 15,
      projectRole: 20,
      ready: true,
    });
  });

  it("narrows to nulls while the identity reads are pending", () => {
    authMocks.me = { data: undefined as never, isPending: true };
    authMocks.workspace = { data: undefined as never, isPending: true };
    authMocks.roles = { data: undefined as never, isPending: true };
    const { result } = renderHook(() => useMemberEditor(SLUG, PROJECT), { wrapper: wrapper(newClient()) });
    expect(result.current).toEqual({ userId: null, workspaceRole: null, projectRole: null, ready: false });
    authMocks.me = { data: { id: "123e4567-e89b-12d3-a456-42661417400a" }, isPending: false };
    authMocks.workspace = { data: { role: 15 }, isPending: false };
    authMocks.roles = { data: { [PROJECT]: 20 }, isPending: false };
  });
});
