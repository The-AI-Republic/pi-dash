// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { issueKeys } from "../api/keys.js";
import { entityPrefsKeyPart } from "../prefs/prefsQuery.js";
import type { IssuesEntity, PrefsRecord } from "../prefs/entities.js";
import {
  isCollapsed,
  readCollapsedGroups,
  toggleCollapsed,
  useToggleCollapsedGroup,
  withCollapsedFilters,
} from "./collapse.js";

vi.mock("../../../shared/shell/index.js", () => ({ notifyError: vi.fn() }));

import { notifyError } from "../../../shared/shell/index.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ENTITY = { scope: "project", projectId: PROJECT } as const;
const ID = "323e4567-e89b-12d3-a456-426614174002";
const USER = "423e4567-e89b-12d3-a456-426614174003";
const WORKSPACE = "523e4567-e89b-12d3-a456-426614174004";

function record(filters: Record<string, unknown> = {}): PrefsRecord {
  return { filters, display_filters: {}, display_properties: {}, rich_filters: {} };
}

function prefsPayload(filters: Record<string, unknown>) {
  return {
    id: ID,
    workspace: WORKSPACE,
    project: PROJECT,
    user: USER,
    filters,
    display_filters: {},
    display_properties: {},
    rich_filters: {},
    preferences: {},
    sort_order: 1,
    created_by: null,
    updated_by: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  };
}

function harness(patchStatus = 200) {
  const calls: Array<{ method: string; body: string }> = [];
  createAppClient("https://api.test", (async (_url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    calls.push({ method, body: init.body ?? "" });
    return {
      status: patchStatus,
      url: "https://api.test",
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(patchStatus < 300 ? prefsPayload({}) : {}),
    };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(
    issueKeys.prefs(SLUG, ...entityPrefsKeyPart(ENTITY)),
    record({ kanban_filters: { group_by: ["todo"], sub_group_by: ["high"] } })
  );
  const wrapper = ({ children }: { children: React.ReactNode }): React.ReactElement =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);
  return { queryClient, calls, wrapper };
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("collapsed group storage", () => {
  it("reads both axes and drops malformed entries", () => {
    expect(readCollapsedGroups({ kanban_filters: { group_by: ["a", 1, null], sub_group_by: "x" } })).toEqual({
      group_by: ["a"],
      sub_group_by: [],
    });
    expect(readCollapsedGroups({})).toEqual({ group_by: [], sub_group_by: [] });
    expect(readCollapsedGroups({ kanban_filters: null })).toEqual({ group_by: [], sub_group_by: [] });
  });

  it("toggles one axis without touching the other", () => {
    const current = { group_by: ["a"], sub_group_by: ["b"] };
    expect(toggleCollapsed(current, "group_by", "c")).toEqual({ group_by: ["a", "c"], sub_group_by: ["b"] });
    expect(toggleCollapsed(current, "group_by", "a")).toEqual({ group_by: [], sub_group_by: ["b"] });
  });

  it("reports membership and writes a complete kanban filter", () => {
    const collapsed = { group_by: ["a"], sub_group_by: [] };
    expect(isCollapsed(collapsed, "group_by", "a")).toBe(true);
    expect(isCollapsed(collapsed, "group_by", "b")).toBe(false);
    expect(withCollapsedFilters({ other: 1, kanban_filters: { stale: true } }, collapsed)).toEqual({
      other: 1,
      kanban_filters: collapsed,
    });
  });
});

describe("useToggleCollapsedGroup", () => {
  it("optimistically adds the group and patches the full record", async () => {
    const h = harness();
    const { result } = renderHook(() => useToggleCollapsedGroup(SLUG, ENTITY as IssuesEntity), {
      wrapper: h.wrapper,
    });
    act(() => result.current.toggleCollapsedGroup("group_by", "started"));
    await waitFor(() => expect(h.calls.some((call) => call.method === "PATCH")).toBe(true));
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({
      filters: { kanban_filters: { group_by: ["todo", "started"], sub_group_by: ["high"] } },
    });
  });

  it("removes an existing key", async () => {
    const h = harness();
    const { result } = renderHook(() => useToggleCollapsedGroup(SLUG, ENTITY as IssuesEntity), {
      wrapper: h.wrapper,
    });
    act(() => result.current.toggleCollapsedGroup("group_by", "todo"));
    await waitFor(() => expect(h.calls.some((call) => call.method === "PATCH")).toBe(true));
    expect(JSON.parse(h.calls.find((call) => call.method === "PATCH")?.body ?? "{}")).toEqual({
      filters: { kanban_filters: { group_by: [], sub_group_by: ["high"] } },
    });
  });

  it("rolls the optimistic state back and notifies on refusal", async () => {
    const h = harness(500);
    const key = issueKeys.prefs(SLUG, ...entityPrefsKeyPart(ENTITY));
    const { result } = renderHook(() => useToggleCollapsedGroup(SLUG, ENTITY as IssuesEntity), {
      wrapper: h.wrapper,
    });
    act(() => result.current.toggleCollapsedGroup("sub_group_by", "low"));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(h.queryClient.getQueryData(key)).toMatchObject({
      filters: { kanban_filters: { group_by: ["todo"], sub_group_by: ["high"] } },
    });
  });
});
