// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { LEAVE_GUARD_ATTR, SELECTION_ROW_ATTR, useBulkSelection } from "./useBulkSelection.js";
import type { SelectionItem } from "./model.js";

const ITEMS: SelectionItem[] = [
  { id: "a", groupKey: "todo" },
  { id: "b", groupKey: "todo" },
  { id: "c", groupKey: "done" },
];

describe("useBulkSelection", () => {
  it("toggles rows, extends ranges and toggles groups", () => {
    const { result } = renderHook(() => useBulkSelection(ITEMS));
    act(() => result.current.onRowToggle("a", false));
    expect([...result.current.selectedIds]).toEqual(["a"]);
    act(() => result.current.onRowToggle("c", true));
    expect([...result.current.selectedIds]).toEqual(["a", "b", "c"]);
    act(() => result.current.onGroupToggle("todo"));
    expect([...result.current.selectedIds]).toEqual(["c"]);
  });

  it("clears the selection", () => {
    const { result } = renderHook(() => useBulkSelection(ITEMS));
    act(() => result.current.onRowToggle("a", false));
    act(() => result.current.onRowToggle("a", false));
    act(() => result.current.clear());
    expect(result.current.selectedIds.size).toBe(0);
  });

  it("moves the active row with arrows and adds with shift+arrow", () => {
    const { result } = renderHook(() => useBulkSelection(ITEMS));
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    });
    expect(result.current.activeId).toBe("a");
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", shiftKey: true }));
    });
    expect(result.current.activeId).toBe("b");
    expect(result.current.isSelected("b")).toBe(true);
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", shiftKey: true }));
    });
    expect(result.current.isSelected("a")).toBe(true);
  });

  it("ignores arrows typed inside form controls", () => {
    const { result } = renderHook(() => useBulkSelection(ITEMS));
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.focus();
    act(() => {
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    });
    expect(result.current.activeId).toBeNull();
    input.remove();
  });

  it.each([
    ["textarea", () => document.createElement("textarea")],
    ["select", () => document.createElement("select")],
    [
      "a direct isContentEditable hit",
      () => {
        const host = document.createElement("div");
        Object.defineProperty(host, "isContentEditable", { value: true });
        return host;
      },
    ],
  ])("ignores arrows typed inside a %s", (_name, makeTarget) => {
    const { result } = renderHook(() => useBulkSelection(ITEMS));
    const target = makeTarget();
    document.body.appendChild(target);
    target.focus();
    act(() => {
      target.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    });
    expect(result.current.activeId).toBeNull();
    target.remove();
  });

  it("does nothing when selection is disabled", () => {
    const { result } = renderHook(() => useBulkSelection(ITEMS, false));
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    });
    expect(result.current.activeId).toBeNull();
    expect(result.current.groupState("todo")).toBe("empty");
    act(() => result.current.onRowToggle("a", false));
    expect(result.current.isSelected("a")).toBe(true);
  });

  it("prunes selected rows that leave the list", () => {
    const { result, rerender } = renderHook(({ items }) => useBulkSelection(items), { initialProps: { items: ITEMS } });
    act(() => result.current.onRowToggle("c", false));
    rerender({ items: ITEMS.slice(0, 2) });
    expect([...result.current.selectedIds]).toEqual([]);
  });

  it("drops an active row that leaves the list", () => {
    const { result, rerender } = renderHook(({ items }) => useBulkSelection(items), { initialProps: { items: ITEMS } });
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    });
    expect(result.current.activeId).toBe("a");
    rerender({ items: ITEMS.slice(1) });
    expect(result.current.activeId).toBeNull();
  });

  it("ignores letter keys", () => {
    const { result } = renderHook(() => useBulkSelection(ITEMS));
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "a" }));
    });
    expect(result.current.activeId).toBeNull();
  });

  it("reports empty group state while selection is off", () => {
    const { result } = renderHook(() => useBulkSelection(ITEMS, false));
    act(() => result.current.onRowToggle("a", false));
    expect(result.current.groupState("todo")).toBe("empty");
  });

  it("keeps a null active row when the list changes", () => {
    const { result, rerender } = renderHook(({ items }) => useBulkSelection(items), { initialProps: { items: ITEMS } });
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    });
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    });
    expect(result.current.activeId).toBe("b");
    rerender({ items: ITEMS.slice(0, 1) });
    expect(result.current.activeId).toBeNull();
  });

  it("reports group state from selected rows", () => {
    const { result } = renderHook(() => useBulkSelection(ITEMS));
    act(() => result.current.onRowToggle("a", false));
    expect(result.current.groupState("todo")).toBe("partial");
  });

  it("keeps a still-present active row after a regroupl", () => {
    const { result, rerender } = renderHook(({ items }) => useBulkSelection(items), { initialProps: { items: ITEMS } });
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    });
    rerender({ items: [...ITEMS] });
    expect(result.current.activeId).toBe("a");
    expect(result.current.isActive("a")).toBe(true);
  });

  it("arms beforeunload while a selection exists and reveals the active row", () => {
    const node = document.createElement("div");
    node.setAttribute(SELECTION_ROW_ATTR, "b");
    node.scrollIntoView = vi.fn();
    document.body.appendChild(node);
    const { result } = renderHook(() => useBulkSelection(ITEMS));
    const event = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    act(() => result.current.onRowToggle("a", false));
    const blocked = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(blocked);
    expect(blocked.defaultPrevented).toBe(true);
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    });
    expect(node.scrollIntoView).toHaveBeenCalled();
    node.remove();
  });

  it("publishes the armed leave guard on the root only while a selection exists", () => {
    const { result } = renderHook(() => useBulkSelection(ITEMS));
    expect(document.documentElement.hasAttribute(LEAVE_GUARD_ATTR)).toBe(false);
    act(() => result.current.onRowToggle("a", false));
    expect(document.documentElement.getAttribute(LEAVE_GUARD_ATTR)).toBe("armed");
    act(() => result.current.clear());
    expect(document.documentElement.hasAttribute(LEAVE_GUARD_ATTR)).toBe(false);
  });

  it("delegates reveal to the surface when one is supplied", () => {
    const reveal = vi.fn();
    renderHook(() => useBulkSelection(ITEMS, true, reveal));
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    });
    expect(reveal).toHaveBeenCalledWith("a");
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    });
    expect(reveal).toHaveBeenLastCalledWith("b");
  });
});
