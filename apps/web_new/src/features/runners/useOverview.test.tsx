// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Overview data wiring (NEWFRONT-540, RUN-002/RUN-010). The hook reads the
// area context, loads the list only when allowed, narrows the rows through
// the pod filter, and hands the screen the two writers.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import * as React from "react";
import type { Pod, Runner } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const allowed = { value: true };
const view = { projectId: "p-1" as string | undefined };
const deleteMutate = vi.fn(async () => undefined);
const revokeMutate = vi.fn(async () => undefined);
const useRunners = vi.fn();
const podsState = {
  data: [{ id: "p-1", name: "WEB_alpha" }] as unknown as Pod[] | undefined,
  isPending: false,
  isError: false,
  isSuccess: true,
};

vi.mock("./area.js", () => ({
  useRunnersArea: () => ({
    scope: { workspaceSlug: "acme", projectId: view.projectId },
    workspaceId: "ws-id",
    access: allowed.value ? "allowed" : "denied",
    allowed: allowed.value,
    base: view.projectId === undefined ? "/acme/runners" : `/acme/projects/${view.projectId}/runners`,
    path: (child?: string) =>
      view.projectId === undefined
        ? child === undefined
          ? "/acme/runners"
          : `/acme/runners/${child}`
        : child === undefined
          ? `/acme/projects/${view.projectId}/runners`
          : `/acme/projects/${view.projectId}/runners/${child}`,
  }),
}));
vi.mock("./api/queries.js", () => ({
  useRunners: (...args: unknown[]) => useRunners(...args),
  useRunnersPods: () => podsState,
}));
vi.mock("./api/mutations.js", () => ({
  useDeleteRunner: () => ({ mutateAsync: deleteMutate }),
  useRevokeRunner: () => ({ mutateAsync: revokeMutate }),
}));

const { useRunnersOverview } = await import("./useOverview.js");

function wrapper({ children }: { children: React.ReactNode }): React.ReactElement {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  return (
    <QueryClientProvider client={queryClient}>
      <React.Fragment>{children}</React.Fragment>
    </QueryClientProvider>
  );
}

beforeEach(() => {
  allowed.value = true;
  view.projectId = "p-1";
  podsState.data = [{ id: "p-1", name: "WEB_alpha" }] as unknown as Pod[];
  podsState.isPending = false;
  podsState.isError = false;
  podsState.isSuccess = true;
  useRunners.mockReset();
  deleteMutate.mockClear();
  revokeMutate.mockClear();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("useRunnersOverview", () => {
  it("reads the project-scoped list only when allowed", () => {
    useRunners.mockReturnValue({ data: [] });
    const { result } = renderHook(() => useRunnersOverview("acme"), { wrapper });
    expect(useRunners).toHaveBeenCalledWith("acme", "ws-id", "p-1", true);
    expect(result.current.runners).toEqual([]);
  });

  it("reads the workspace aggregate when the area has no project scope", () => {
    view.projectId = undefined;
    useRunners.mockReturnValue({ data: [] });
    renderHook(() => useRunnersOverview("acme"), { wrapper });
    expect(useRunners).toHaveBeenCalledWith("acme", "ws-id", null, true);
  });

  it("passes the denied state to the list read", () => {
    allowed.value = false;
    useRunners.mockReturnValue({ data: undefined });
    renderHook(() => useRunnersOverview("acme"), { wrapper });
    expect(useRunners).toHaveBeenCalledWith("acme", "ws-id", "p-1", false);
  });

  it("narrows rows and resolves the writers", async () => {
    useRunners.mockReturnValue({
      data: [
        { id: "r-1", name: "alpha", pod: "p-1" },
        { id: "r-2", name: "beta", pod: "p-2" },
      ] as unknown as Runner[],
    });
    const { result } = renderHook(() => useRunnersOverview("acme"), { wrapper });
    expect(result.current.runners.map((row) => row.id)).toEqual(["r-1", "r-2"]);
    await result.current.onDelete("r-1");
    expect(deleteMutate).toHaveBeenCalledWith("r-1");
    await result.current.onRevoke("r-2");
    expect(revokeMutate).toHaveBeenCalledWith("r-2");
  });

  it("hands the screen the pods and their read progress", () => {
    useRunners.mockReturnValue({ data: [] });
    const { result } = renderHook(() => useRunnersOverview("acme"), { wrapper });
    expect(result.current.pods).toEqual([{ id: "p-1", name: "WEB_alpha" }]);
    expect(result.current.podsLoading).toBe(false);
    expect(result.current.podsFailed).toBe(false);
  });

  it("reports the pod read while it is still pending", () => {
    useRunners.mockReturnValue({ data: [] });
    podsState.data = undefined;
    podsState.isPending = true;
    podsState.isSuccess = false;
    const { result } = renderHook(() => useRunnersOverview("acme"), { wrapper });
    expect(result.current.pods).toBeUndefined();
    expect(result.current.podsLoading).toBe(true);
  });

  it("reports a failed pod read", () => {
    useRunners.mockReturnValue({ data: [] });
    podsState.isError = true;
    podsState.isSuccess = false;
    const { result } = renderHook(() => useRunnersOverview("acme"), { wrapper });
    expect(result.current.podsFailed).toBe(true);
  });

  it("reports neither progress nor failure while denied", () => {
    allowed.value = false;
    useRunners.mockReturnValue({ data: undefined });
    podsState.isError = true;
    podsState.isSuccess = false;
    const { result } = renderHook(() => useRunnersOverview("acme"), { wrapper });
    expect(result.current.podsLoading).toBe(false);
    expect(result.current.podsFailed).toBe(false);
  });

  it("ignores the list while denied", () => {
    allowed.value = false;
    useRunners.mockReturnValue({ data: [{ id: "r-1", name: "alpha", pod: "p-1" }] as unknown as Runner[] });
    const { result } = renderHook(() => useRunnersOverview("acme"), { wrapper });
    expect(result.current.runners).toEqual([]);
  });
});
