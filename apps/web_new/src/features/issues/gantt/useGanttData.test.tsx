// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline data side (parity/specs/issues.md §6 and §14, ISS-044/049):
// the per-project state read that tints bars, and row activation which
// opens the shared peek on a wide viewport or leaves for the browse route
// on a narrow one.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { State } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import type { GanttRow } from "./rows.js";
import { ganttStatesKey, stateColorOf, useGanttPeek, useGanttStates } from "./useGanttData.js";

const navigate = vi.fn();

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => navigate,
}));

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const STATE_ID = "323e4567-e89b-12d3-a456-426614174002";
const STATE_TWO = "423e4567-e89b-12d3-a456-426614174003";

function state(overrides: Partial<State> = {}): State {
  return {
    id: STATE_ID,
    name: "Backlog",
    color: "#00ff00",
    group: "backlog",
    sequence: 1,
    project_id: PROJECT,
    workspace_id: "123e4567-e89b-12d3-a456-426614174000",
    description: "",
    default: false,
    ...overrides,
  };
}

function row(overrides: Partial<GanttRow> = {}): GanttRow {
  return {
    id: "row-1",
    identifier: "WEB-1",
    name: "First",
    startIso: null,
    targetIso: null,
    durationDays: null,
    stateId: null,
    sortOrder: 1,
    priority: "none",
    sequenceId: 1,
    archived: false,
    unconfirmed: false,
    ...overrides,
  };
}

function stubViewport(narrow: boolean): void {
  vi.stubGlobal("matchMedia", () => ({
    matches: narrow,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
}

function wrapper() {
  createAppClient("https://api.test", (() => {
    throw new Error("unexpected fetch in a seeded render");
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  function Host({ children, seed }: { children?: unknown; seed?: unknown }): React.ReactElement {
    if (seed !== undefined) queryClient.setQueryData(ganttStatesKey(SLUG, PROJECT), seed);
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  }
  return { queryClient, Host };
}

afterEach(() => {
  resetAppClient();
  navigate.mockReset();
  vi.unstubAllGlobals();
});

describe("stateColorOf", () => {
  it("finds the row's state color", () => {
    expect(stateColorOf([state(), state({ id: STATE_TWO, color: "#0000ff" })], STATE_TWO)).toBe("#0000ff");
  });

  it("returns null for an unknown or missing state", () => {
    expect(stateColorOf([state()], "other")).toBeNull();
    expect(stateColorOf([state()], null)).toBeNull();
  });
});

describe("ganttStatesKey", () => {
  it("scopes the read per workspace and project", () => {
    expect(ganttStatesKey(SLUG, PROJECT)).toEqual(["ws", SLUG, "issues", "gantt-states", PROJECT]);
  });
});

describe("useGanttStates", () => {
  it("reads the cached project states", () => {
    const { Host } = wrapper();
    const seeded = [state()];
    const { result } = renderHook(() => useGanttStates(SLUG, PROJECT), {
      wrapper: ({ children }) => <Host seed={seeded}>{children}</Host>,
    });
    expect(result.current).toEqual(seeded);
  });

  it("reports an empty list while the read is unresolved", () => {
    const { Host } = wrapper();
    const { result } = renderHook(() => useGanttStates(SLUG, PROJECT), {
      wrapper: ({ children }) => <Host>{children}</Host>,
    });
    expect(result.current).toEqual([]);
  });

  it("loads the states through the app client when nothing is cached", async () => {
    const requests: string[] = [];
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    createAppClient("https://api.test", (async (url: string) => {
      requests.push(url);
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify([state()]),
      };
    }) as never);
    function Host({ children }: { children?: unknown }): React.ReactElement {
      return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
    }
    const { result } = renderHook(() => useGanttStates(SLUG, PROJECT), {
      wrapper: ({ children }) => <Host>{children}</Host>,
    });
    await waitFor(() => expect(result.current).toHaveLength(1), { timeout: 2_000 }).catch(() => {
      throw new Error(`states not loaded: ${JSON.stringify(requests)}`);
    });
    expect(requests[0]).toContain(`/projects/${PROJECT}/states/`);
  });

  it("opens the shared peek on a wide viewport and closes it", () => {
    stubViewport(false);
    const { result } = renderHook(() => useGanttPeek(SLUG));
    expect(result.current.peek).toBeNull();
    act(() => result.current.openRow(row()));
    expect(result.current.peek).toEqual({ row: expect.objectContaining({ id: "row-1" }) });
    expect(navigate).not.toHaveBeenCalled();
    act(() => result.current.closePeek());
    expect(result.current.peek).toBeNull();
  });

  it("does nothing for an unconfirmed row on either viewport", () => {
    for (const narrow of [false, true]) {
      stubViewport(narrow);
      const { result } = renderHook(() => useGanttPeek(SLUG));
      act(() => result.current.openRow(row({ id: "temp-abc", identifier: "WEB-0", unconfirmed: true })));
      expect(result.current.peek).toBeNull();
      expect(navigate).not.toHaveBeenCalled();
    }
  });

  it("leaves for the browse route on a narrow viewport", () => {
    stubViewport(true);
    const { result } = renderHook(() => useGanttPeek(SLUG));
    act(() => result.current.openRow(row({ identifier: "WEB-9" })));
    expect(navigate).toHaveBeenCalledWith({
      to: "/$ws/browse/$issueKey",
      params: { ws: SLUG, issueKey: "WEB-9" },
    });
    expect(result.current.peek).toBeNull();
  });
});
