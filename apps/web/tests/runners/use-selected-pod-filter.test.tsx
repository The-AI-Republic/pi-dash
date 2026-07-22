/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { IPod, IRunner } from "@pi-dash/types";
import { useSelectedPodFilter } from "@/hooks/use-selected-pod-filter";

function makePod(id: string, name = id): IPod {
  return {
    id,
    name,
    description: "",
    is_default: false,
    workspace: "ws-1",
    project: "proj-1",
    project_identifier: "WEB",
    created_by: null,
    runner_count: 0,
    created_at: "",
    updated_at: "",
  };
}

function makeRunner(id: string, podId: string): IRunner {
  return {
    id,
    name: id,
    status: "online",
    os: "linux",
    arch: "x86_64",
    runner_version: "1.0.0",
    working_dir: "",
    protocol_version: 1,
    capabilities: [],
    last_heartbeat_at: null,
    owner: null,
    pod: podId,
    pod_detail: { id: podId, name: podId, is_default: false, project: "proj-1", project_identifier: "WEB" },
    dev_machine_detail: null,
    connection: "conn-1",
    enrolled_at: null,
    revoked_at: null,
    revoked_reason: "",
    created_at: "",
    updated_at: "",
  };
}

describe("useSelectedPodFilter", () => {
  const pods = [makePod("pod-a"), makePod("pod-b")];
  const runners = [makeRunner("r1", "pod-a"), makeRunner("r2", "pod-a"), makeRunner("r3", "pod-b")];

  it("returns all runners when no pod is selected", () => {
    const { result } = renderHook(() => useSelectedPodFilter(runners, pods));
    expect(result.current.selectedPodId).toBeNull();
    expect(result.current.selectedPod).toBeUndefined();
    expect(result.current.filteredRunners).toEqual(runners);
  });

  it("filters runners to the selected pod", () => {
    const { result } = renderHook(() => useSelectedPodFilter(runners, pods));

    act(() => {
      result.current.setSelectedPodId("pod-a");
    });

    expect(result.current.selectedPodId).toBe("pod-a");
    expect(result.current.selectedPod).toEqual(pods[0]);
    expect(result.current.filteredRunners?.map((r) => r.id)).toEqual(["r1", "r2"]);
  });

  it("returns an empty list when the selected pod has no runners", () => {
    const { result } = renderHook(() => useSelectedPodFilter([makeRunner("r1", "pod-a")], pods));

    act(() => {
      result.current.setSelectedPodId("pod-b");
    });

    expect(result.current.filteredRunners).toEqual([]);
  });

  it("clears selection when setSelectedPodId(null) is called (toggle off)", () => {
    const { result } = renderHook(() => useSelectedPodFilter(runners, pods));

    act(() => {
      result.current.setSelectedPodId("pod-a");
    });
    expect(result.current.selectedPodId).toBe("pod-a");

    act(() => {
      result.current.setSelectedPodId(null);
    });
    expect(result.current.selectedPodId).toBeNull();
    expect(result.current.filteredRunners).toEqual(runners);
  });

  it("self-heals: clears selection when the chosen pod is removed from the pods list", () => {
    const { result, rerender } = renderHook(({ p }: { p: IPod[] }) => useSelectedPodFilter(runners, p), {
      initialProps: { p: pods },
    });

    act(() => {
      result.current.setSelectedPodId("pod-a");
    });
    expect(result.current.selectedPodId).toBe("pod-a");

    // pod-a deleted server-side; SWR refresh hands us a list without it.
    rerender({ p: [makePod("pod-b")] });

    expect(result.current.selectedPodId).toBeNull();
    expect(result.current.filteredRunners).toEqual(runners);
  });

  it("does NOT clear selection while pods is undefined (loading)", () => {
    const { result, rerender } = renderHook(({ p }: { p: IPod[] | undefined }) => useSelectedPodFilter(runners, p), {
      initialProps: { p: pods as IPod[] | undefined },
    });

    act(() => {
      result.current.setSelectedPodId("pod-a");
    });

    // Simulate the SWR cache being cleared mid-flight; we should keep the
    // user's selection rather than treat "not yet loaded" as "deleted".
    rerender({ p: undefined });

    expect(result.current.selectedPodId).toBe("pod-a");
  });

  it("returns undefined runners when runners are still loading", () => {
    const { result } = renderHook(() => useSelectedPodFilter(undefined, pods));
    expect(result.current.filteredRunners).toBeUndefined();
  });
});
