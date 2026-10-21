// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Publish-settings helpers (parity/specs/project-settings-lifecycle.md
// §PSET-016): published detection, form seeding, dirty math, link base and
// the create/update/unpublish mutations.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor, type RenderHookOptions } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProjectDeployBoardStatus } from "@pidash/api-client";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import {
  boardFormDirty,
  boardFormFrom,
  isBoardPublished,
  publishedBoardLink,
  PUBLISHED_TITLE,
  PUBLISH_FAILED_TITLE,
  UNPUBLISHED_TITLE,
  usePublishBoard,
  useUnpublishBoard,
  useUpdateBoard,
} from "./publish.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const BOARD_ID = "323e4567-e89b-12d3-a456-426614174002";

function board(overrides: Record<string, unknown> = {}): ProjectDeployBoardStatus {
  return {
    id: BOARD_ID,
    project_details: {
      id: PROJECT_ID,
      identifier: "WEB",
      name: "Web",
      cover_image: null,
      cover_image_url: null,
      logo_props: {},
      description: "",
      is_default: false,
    },
    workspace_detail: { id: "123e4567-e89b-12d3-a456-426614174000", name: "Acme", slug: "acme", logo_url: null },
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    project: PROJECT_ID,
    entity_identifier: null,
    entity_name: "project",
    anchor: "live-token",
    is_comments_enabled: false,
    is_reactions_enabled: false,
    intake: null,
    is_votes_enabled: false,
    view_props: { list: true, kanban: true },
    is_activity_enabled: false,
    is_disabled: false,
    ...overrides,
  } as ProjectDeployBoardStatus;
}

function emptyBoard(): ProjectDeployBoardStatus {
  return {
    deleted_at: null,
    entity_identifier: null,
    entity_name: "",
    is_comments_enabled: false,
    is_reactions_enabled: false,
    is_votes_enabled: false,
    view_props: null,
    is_activity_enabled: false,
    is_disabled: false,
    created_by: null,
    updated_by: null,
    intake: null,
  };
}

function stub(status = 200, body: unknown = board()) {
  const calls: string[] = [];
  createAppClient("https://api.test", (async (url: string, init?: { method?: string; body?: string }) => {
    calls.push(`${init?.method ?? "GET"} ${url}`);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    return {
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(status >= 200 && status < 300 ? body : { detail: "refused" }),
    };
  }) as never);
  return calls;
}

type WrapperComponent = NonNullable<RenderHookOptions<unknown>["wrapper"]>;

// @tanstack/react-query carries React 18's children type; retype it for this app's React 19.
const Provider = QueryClientProvider as unknown as React.ComponentType<{
  client: QueryClient;
  children?: React.ReactNode;
}>;

function wrapper(): WrapperComponent {
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false }, queries: { retry: false } } });
  return function Wrapper({ children }) {
    return <Provider client={client}>{children}</Provider>;
  };
}

beforeEach(() => vi.clearAllMocks());
afterEach(() => resetAppClient());

describe("publish form math", () => {
  it("detects a published board by its anchor", () => {
    expect(isBoardPublished(board())).toBe(true);
    expect(isBoardPublished(board({ anchor: null }))).toBe(false);
    expect(isBoardPublished(emptyBoard())).toBe(false);
    expect(isBoardPublished(undefined)).toBe(false);
  });

  it("defaults to both layouts and no interaction toggles", () => {
    expect(boardFormFrom(undefined)).toEqual({
      list: true,
      kanban: true,
      comments: false,
      reactions: false,
      voting: false,
    });
    expect(boardFormFrom(emptyBoard())).toEqual({
      list: true,
      kanban: true,
      comments: false,
      reactions: false,
      voting: false,
    });
  });

  it("reads stored layouts and toggles", () => {
    expect(
      boardFormFrom(
        board({ view_props: { list: false, kanban: true }, is_comments_enabled: true, is_votes_enabled: true })
      )
    ).toEqual({ list: false, kanban: true, comments: true, reactions: false, voting: true });
  });

  it("is dirty only when the form differs from the stored row", () => {
    const stored = board();
    expect(boardFormDirty(boardFormFrom(stored), stored)).toBe(false);
    expect(boardFormDirty({ ...boardFormFrom(stored), voting: true }, stored)).toBe(true);
  });

  it("builds the public link from the spaces base path", () => {
    expect(publishedBoardLink("abc")).toContain("/issues/abc");
  });

  it("uses a configured spaces origin and path when provided", () => {
    vi.stubEnv("VITE_SPACE_BASE_URL", "https://space.test");
    vi.stubEnv("VITE_SPACE_BASE_PATH", "/public");
    expect(publishedBoardLink("abc")).toBe("https://space.test/public/issues/abc");
    vi.unstubAllEnvs();
  });
});

describe("publish mutations", () => {
  it("publishes, toasts and re-reads the board", async () => {
    const calls = stub();
    const { result } = renderHook(() => usePublishBoard(), { wrapper: wrapper() });
    result.current.mutate({ workspaceSlug: "acme", projectId: PROJECT_ID, input: { views: { list: true } } });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(notifySuccess).toHaveBeenCalledWith(PUBLISHED_TITLE, expect.any(String));
    expect(calls.some((call) => call.startsWith("POST") && call.includes("project-deploy-boards"))).toBe(true);
  });

  it("toasts a failed publish and rejects", async () => {
    stub(400);
    const { result } = renderHook(() => usePublishBoard(), { wrapper: wrapper() });
    result.current.mutate({ workspaceSlug: "acme", projectId: PROJECT_ID, input: { views: { list: true } } });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith(PUBLISH_FAILED_TITLE, expect.any(String));
  });

  it("updates an existing board", async () => {
    const calls = stub();
    const { result } = renderHook(() => useUpdateBoard(), { wrapper: wrapper() });
    result.current.mutate({
      workspaceSlug: "acme",
      projectId: PROJECT_ID,
      boardId: BOARD_ID,
      input: { is_votes_enabled: false },
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls.some((call) => call.startsWith("PATCH") && call.includes(BOARD_ID))).toBe(true);
  });

  it("unpublishes with its own toast", async () => {
    stub(204, {});
    const { result } = renderHook(() => useUnpublishBoard(), { wrapper: wrapper() });
    result.current.mutate({ workspaceSlug: "acme", projectId: PROJECT_ID, boardId: BOARD_ID });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(notifySuccess).toHaveBeenCalledWith(UNPUBLISHED_TITLE, expect.any(String));
  });
});
