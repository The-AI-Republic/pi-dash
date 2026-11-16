// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const navigation = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({ useNavigate: () => navigation.navigate }));

import type { SpaceBoardSearch } from "../lib/address.js";
import { useSettledSpaceLayout } from "./useSettledSpaceLayout.js";

const ID = "11111111-1111-4111-8111-111111111111";

function settle(search: SpaceBoardSearch, viewProps: Record<string, boolean> | undefined) {
  return renderHook(
    (props: { search: SpaceBoardSearch }) => useSettledSpaceLayout("anchor-1", props.search, viewProps),
    {
      initialProps: { search },
    }
  );
}

afterEach(() => {
  navigation.navigate.mockReset();
});

describe("useSettledSpaceLayout (SPC-020–022)", () => {
  it("keeps a requested layout the board offers without touching the address", () => {
    const { result } = settle({ board: "kanban" }, { list: true, kanban: true });
    expect(result.current.active).toBe("kanban");
    expect(result.current.offered).toEqual(["list", "kanban"]);
    expect(navigation.navigate).not.toHaveBeenCalled();
  });

  it("settles a missing layout on the first offered one and replaces the address", () => {
    const { result } = settle({ peekId: "p1", state: ID }, { list: true, kanban: true });
    expect(result.current.active).toBe("list");
    expect(navigation.navigate).toHaveBeenCalledWith({
      href: `/spaces/issues/anchor-1/?board=list&peekId=p1&state=${ID}`,
      replace: true,
    });
  });

  it("falls back to the offered layout when the address names one the publisher disabled", () => {
    const { result } = settle({ board: "list" }, { kanban: true });
    expect(result.current.active).toBe("kanban");
    expect(result.current.offered).toEqual(["kanban"]);
    expect(navigation.navigate).toHaveBeenCalledWith({ href: "/spaces/issues/anchor-1/?board=kanban", replace: true });
  });

  it("settles unknown values on list when settings carry no view props", () => {
    settle({ board: "kanban" }, undefined);
    expect(navigation.navigate).toHaveBeenCalledWith({ href: "/spaces/issues/anchor-1/?board=list", replace: true });
  });

  it("pushes exactly one address when the visitor picks another offered layout", () => {
    const { result } = settle({ board: "list", peekId: "p1", priority: "high" }, { list: true, kanban: true });
    act(() => result.current.select("kanban"));
    expect(navigation.navigate).toHaveBeenCalledTimes(1);
    expect(navigation.navigate).toHaveBeenCalledWith({
      href: "/spaces/issues/anchor-1/?board=kanban&peekId=p1&priority=high",
    });
  });

  it("ignores a pick of the layout already shown", () => {
    const { result } = settle({ board: "list" }, { list: true, kanban: true });
    act(() => result.current.select("list"));
    expect(navigation.navigate).not.toHaveBeenCalled();
  });

  it("does not re-issue the fallback rewrite when the parent re-renders", () => {
    const { rerender } = settle({}, { list: true, kanban: true });
    expect(navigation.navigate).toHaveBeenCalledTimes(1);
    rerender({ search: {} });
    expect(navigation.navigate).toHaveBeenCalledTimes(1);
  });

  it("settles again when the address changes under it", () => {
    const { rerender } = settle({ board: "list" }, { list: true, kanban: true });
    expect(navigation.navigate).not.toHaveBeenCalled();
    rerender({ search: { board: "bogus" as never } });
    expect(navigation.navigate).toHaveBeenCalledWith({ href: "/spaces/issues/anchor-1/?board=list", replace: true });
  });
});
