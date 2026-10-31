// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const navigation = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({ useNavigate: () => navigation.navigate }));

import type { SpaceBoardFilters, SpaceBoardSearch } from "../lib/address.js";
import { useSpaceFilterNav } from "./useSpaceFilterNav.js";

const STATE_A = "11111111-1111-4111-8111-111111111111";
const STATE_B = "22222222-2222-4222-8222-222222222222";
const LABEL = "33333333-3333-4333-8333-333333333333";

function renderNav(search: SpaceBoardSearch, filters: SpaceBoardFilters) {
  return renderHook(() => useSpaceFilterNav("anchor-1", search, filters));
}

afterEach(() => {
  navigation.navigate.mockReset();
});

describe("useSpaceFilterNav (SPC-035/036)", () => {
  it("pushes a picked value under the singular key, keeping the view and the open issue", () => {
    const { result } = renderNav({ board: "list", peekId: "p1" }, {});
    act(() => result.current.toggleValue("priority", "high"));
    expect(navigation.navigate).toHaveBeenCalledTimes(1);
    expect(navigation.navigate).toHaveBeenCalledWith({
      href: "/spaces/issues/anchor-1/?board=list&peekId=p1&priority=high",
    });
  });

  it("appends a second value to the same comma-joined key", () => {
    const { result } = renderNav({ board: "list", priority: "high" }, { priority: "high" });
    act(() => result.current.toggleValue("priority", "urgent"));
    expect(navigation.navigate).toHaveBeenCalledWith({
      href: "/spaces/issues/anchor-1/?board=list&priority=high%2Curgent",
    });
  });

  it("drops a picked value that is already applied", () => {
    const { result } = renderNav({ board: "kanban", state: STATE_A }, { state: STATE_A });
    act(() => result.current.toggleValue("state", STATE_A));
    expect(navigation.navigate).toHaveBeenCalledWith({ href: "/spaces/issues/anchor-1/?board=kanban" });
  });

  it("keeps surviving state values under the singular state key on pill removal", () => {
    const { result } = renderNav(
      { board: "list", state: [STATE_A, STATE_B].join(",") },
      { state: [STATE_A, STATE_B].join(",") }
    );
    act(() => result.current.removeValue("state", STATE_A));
    const href = navigation.navigate.mock.calls[0]?.[0]?.href as string;
    expect(new URL(href, "https://board.test").searchParams.get("state")).toBe(STATE_B);
    expect(href).not.toContain("states=");
  });

  it("removes one priority value and keeps the other facets", () => {
    const { result } = renderNav(
      { board: "list", priority: "high,urgent", labels: LABEL },
      { priority: "high,urgent", labels: LABEL }
    );
    act(() => result.current.removeValue("priority", "high"));
    expect(navigation.navigate).toHaveBeenCalledWith({
      href: `/spaces/issues/anchor-1/?board=list&priority=urgent&labels=${LABEL}`,
    });
  });

  it("clears one facet and keeps the others", () => {
    const { result } = renderNav(
      { board: "list", priority: "high", state: STATE_A },
      { priority: "high", state: STATE_A }
    );
    act(() => result.current.clearFacet("priority"));
    expect(navigation.navigate).toHaveBeenCalledWith({
      href: `/spaces/issues/anchor-1/?board=list&state=${STATE_A}`,
    });
  });

  it("clear-all strips every facet key and keeps the view and the open issue", () => {
    const { result } = renderNav(
      { board: "kanban", peekId: "p1", priority: "high", state: STATE_A, labels: LABEL },
      { priority: "high", state: STATE_A, labels: LABEL }
    );
    act(() => result.current.clearAll());
    expect(navigation.navigate).toHaveBeenCalledWith({ href: "/spaces/issues/anchor-1/?board=kanban&peekId=p1" });
  });
});
