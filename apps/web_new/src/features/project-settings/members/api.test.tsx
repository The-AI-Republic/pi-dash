// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members reads (PSET-018/027): the query keys stay under the workspace
// scope, both reads parse through the shared client, and a failure detail
// carries the server message with a fallback.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { ApiError, type ProjectMemberList, type WorkspaceMemberList } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: {
    listMembers: vi.fn(),
    listWorkspaceMembers: vi.fn(),
  },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
}));

import { notifyError } from "../../../shared/shell/index.js";
import {
  MEMBERS_LOAD_FAILED_FALLBACK,
  MEMBERS_LOAD_FAILED_TITLE,
  memberFailureDetail,
  memberKeys,
  notifyMembersLoadFailure,
  projectMembersQueryOptions,
  useProjectMemberships,
  useWorkspaceDirectory,
  workspaceDirectoryQueryOptions,
} from "./api.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

const MEMBERSHIPS: ProjectMemberList = [
  {
    id: "323e4567-e89b-12d3-a456-426614174001",
    role: 20,
    member: "123e4567-e89b-12d3-a456-42661417400a",
    project: PROJECT,
    original_role: 20,
    created_at: "2025-01-01T00:00:00Z",
  },
];

const DIRECTORY: WorkspaceMemberList = [
  {
    id: "423e4567-e89b-12d3-a456-426614174001",
    role: 15,
    member: {
      id: "123e4567-e89b-12d3-a456-42661417400a",
      first_name: "Ada",
      last_name: "Lovelace",
      avatar: "",
      avatar_url: null,
      display_name: "ada",
      is_bot: false,
    },
  },
];

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

function newClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

beforeEach(() => {
  createAppClient("https://api.test");
});

afterEach(() => {
  resetAppClient();
  contractMocks.listMembers.mockReset();
  contractMocks.listWorkspaceMembers.mockReset();
  vi.mocked(notifyError).mockReset();
});

describe("member query keys", () => {
  it("scopes both reads to the workspace and separates projects", () => {
    expect(memberKeys.memberships(SLUG, PROJECT)).toEqual([
      "ws",
      SLUG,
      "project-settings",
      "members",
      "memberships",
      PROJECT,
    ]);
    expect(memberKeys.directory(SLUG)).toEqual(["ws", SLUG, "project-settings", "members", "directory"]);
  });

  it("builds options that call the matching contract", async () => {
    contractMocks.listMembers.mockResolvedValue(MEMBERSHIPS);
    contractMocks.listWorkspaceMembers.mockResolvedValue(DIRECTORY);
    const client = createAppClient("https://api.test");
    const queryClient = newClient();
    await queryClient.fetchQuery(projectMembersQueryOptions(client, SLUG, PROJECT));
    await queryClient.fetchQuery(workspaceDirectoryQueryOptions(client, SLUG));
    expect(contractMocks.listMembers).toHaveBeenCalledWith(client, SLUG, PROJECT);
    expect(contractMocks.listWorkspaceMembers).toHaveBeenCalledWith(client, SLUG);
  });
});

describe("member read hooks", () => {
  it("resolves the memberships", async () => {
    contractMocks.listMembers.mockResolvedValue(MEMBERSHIPS);
    const queryClient = newClient();
    const { result } = renderHook(() => useProjectMemberships(SLUG, PROJECT), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual(MEMBERSHIPS);
  });

  it("resolves the directory with an optional email", async () => {
    contractMocks.listWorkspaceMembers.mockResolvedValue([
      { ...DIRECTORY[0]!, member: { ...DIRECTORY[0]!.member, email: "ada@example.test" } },
    ]);
    const queryClient = newClient();
    const { result } = renderHook(() => useWorkspaceDirectory(SLUG), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data?.[0]?.member.email).toBe("ada@example.test");
  });
});

describe("failure detail", () => {
  it("prefers the server message", () => {
    expect(memberFailureDetail(new ApiError({ status: 403, code: "http", message: "Not allowed here." }))).toBe(
      "Not allowed here."
    );
  });

  it("falls back for empty or non-API errors", () => {
    expect(memberFailureDetail(new ApiError({ status: 500, code: "http", message: "" }))).toBe(
      MEMBERS_LOAD_FAILED_FALLBACK
    );
    expect(memberFailureDetail(new Error("boom"))).toBe(MEMBERS_LOAD_FAILED_FALLBACK);
  });

  it("toasts the title and the detail", () => {
    notifyMembersLoadFailure(new ApiError({ status: 500, code: "http", message: "Server fell over." }));
    expect(notifyError).toHaveBeenCalledWith(MEMBERS_LOAD_FAILED_TITLE, "Server fell over.");
    notifyMembersLoadFailure(new Error("boom"));
    expect(notifyError).toHaveBeenLastCalledWith(MEMBERS_LOAD_FAILED_TITLE, MEMBERS_LOAD_FAILED_FALLBACK);
  });
});
