// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pod filter selection (NEWFRONT-540 groundwork, RUN-012). The overview owns
// the client-side filter state; this suite pins the row selection, the tile
// toggle and the self-healing clear.
import { act, renderHook } from "@testing-library/react";
import type { Pod, Runner } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { filterRunnersByPod, selectPod, useRunnerPodFilter } from "./podFilter.js";

const pod = (id: string, overrides: Partial<Pod> = {}): Pod =>
  ({ id, name: id, is_default: false, runner_count: 0, ...overrides }) as Pod;
const runner = (id: string, podId: string): Runner => ({ id, pod: podId }) as Runner;

describe("filterRunnersByPod", () => {
  it("returns every row when no pod is selected", () => {
    const rows = [runner("a", "p-1"), runner("b", "p-2")];
    expect(filterRunnersByPod(rows, null)).toEqual(rows);
    // A fresh array, so callers can't mutate the cache entry.
    expect(filterRunnersByPod(rows, null)).not.toBe(rows);
  });

  it("keeps only the rows of the selected pod", () => {
    const rows = [runner("a", "p-1"), runner("b", "p-2"), runner("c", "p-1")];
    expect(filterRunnersByPod(rows, "p-1").map((row) => row.id)).toEqual(["a", "c"]);
  });
});

describe("selectPod", () => {
  it("resolves the selected row or null", () => {
    const pods = [pod("p-1"), pod("p-2")];
    expect(selectPod(pods, "p-2")?.id).toBe("p-2");
    expect(selectPod(pods, "gone")).toBeNull();
    expect(selectPod(pods, null)).toBeNull();
    expect(selectPod(undefined, "p-1")).toBeNull();
  });
});

describe("useRunnerPodFilter", () => {
  it("toggles a pod on and off and narrows the rows", () => {
    const pods = [pod("p-1"), pod("p-2")];
    const runners = [runner("a", "p-1"), runner("b", "p-2")];
    const { result } = renderHook(() => useRunnerPodFilter({ runners, pods, podsLoaded: true }));
    expect(result.current.runners.map((row) => row.id)).toEqual(["a", "b"]);

    act(() => result.current.toggle("p-1"));
    expect(result.current.selectedPodId).toBe("p-1");
    expect(result.current.selectedPod?.id).toBe("p-1");
    expect(result.current.runners.map((row) => row.id)).toEqual(["a"]);

    // Toggling the active pod clears the filter.
    act(() => result.current.toggle("p-1"));
    expect(result.current.selectedPodId).toBeNull();
    expect(result.current.runners.map((row) => row.id)).toEqual(["a", "b"]);
  });

  it("clears explicitly", () => {
    const { result } = renderHook(() => useRunnerPodFilter({ runners: [], pods: [pod("p-1")], podsLoaded: true }));
    act(() => result.current.toggle("p-1"));
    expect(result.current.selectedPodId).toBe("p-1");
    act(() => result.current.clear());
    expect(result.current.selectedPodId).toBeNull();
  });

  it("heals a vanished pod without letting a refetch blip wipe the filter", () => {
    const { result, rerender } = renderHook(
      (props: { pods: Pod[]; podsLoaded: boolean }) => useRunnerPodFilter({ runners: [runner("a", "p-1")], ...props }),
      { initialProps: { pods: [pod("p-1")], podsLoaded: true } }
    );
    act(() => result.current.toggle("p-1"));
    expect(result.current.selectedPodId).toBe("p-1");

    // A refetch starts before the pod list lands again: no pod row resolves
    // yet, but the rows stay narrowed by the id rather than silently
    // widening to every runner.
    rerender({ pods: [], podsLoaded: false });
    expect(result.current.selectedPod).toBeNull();
    expect(result.current.runners.map((row) => row.id)).toEqual(["a"]);

    // The same pod came back, so the selection survives the blip.
    rerender({ pods: [pod("p-1")], podsLoaded: true });
    expect(result.current.selectedPodId).toBe("p-1");

    // A settled list without the pod clears it for good.
    rerender({ pods: [pod("p-2")], podsLoaded: true });
    expect(result.current.selectedPodId).toBeNull();
    rerender({ pods: [pod("p-1"), pod("p-2")], podsLoaded: true });
    expect(result.current.selectedPodId).toBeNull();
  });

  it("falls back to no pods while the list is undefined mid-refetch", () => {
    const { result, rerender } = renderHook(
      (props: { pods: Pod[] | undefined; podsLoaded: boolean }) =>
        useRunnerPodFilter({ runners: [runner("a", "p-1")], ...props }),
      { initialProps: { pods: [pod("p-1")] as Pod[] | undefined, podsLoaded: true } }
    );
    act(() => result.current.toggle("p-1"));
    rerender({ pods: undefined, podsLoaded: false });
    expect(result.current.selectedPodId).toBeNull();
    expect(result.current.selectedPod).toBeNull();
    expect(result.current.runners.map((row) => row.id)).toEqual(["a"]);
  });

  it("narrows to nothing mid-load when the pods are not there yet", () => {
    const { result } = renderHook(() => useRunnerPodFilter({ runners: undefined, pods: undefined, podsLoaded: false }));
    expect(result.current.runners).toEqual([]);
    expect(result.current.selectedPod).toBeNull();
  });

  it("keeps the selection when the settled list still carries the pod", () => {
    const { result, rerender } = renderHook(
      (props: { pods: Pod[]; podsLoaded: boolean }) => useRunnerPodFilter({ runners: [], ...props }),
      { initialProps: { pods: [pod("p-1")], podsLoaded: true } }
    );
    act(() => result.current.toggle("p-1"));
    rerender({ pods: [pod("p-1"), pod("p-2")], podsLoaded: true });
    expect(result.current.selectedPodId).toBe("p-1");
  });
});
