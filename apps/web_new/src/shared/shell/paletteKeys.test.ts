// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, renderHook } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { usePaletteKeys, type PaletteKeyOptions } from "./paletteKeys.js";

function keyEvent(key: string): React.KeyboardEvent {
  return { key, preventDefault: vi.fn(), stopPropagation: vi.fn() } as unknown as React.KeyboardEvent;
}

function setup(options: Partial<PaletteKeyOptions> = {}) {
  const onActivate = vi.fn();
  const onClearQuery = vi.fn();
  const onDismiss = vi.fn();
  const hook = renderHook((props: PaletteKeyOptions) => usePaletteKeys(props), {
    initialProps: {
      count: 3,
      query: "",
      onActivate,
      onClearQuery,
      onDismiss,
      ...options,
    },
  });
  return { ...hook, onActivate, onClearQuery, onDismiss };
}

function press(keys: { onKeyDown: (event: React.KeyboardEvent) => void }, key: string): void {
  act(() => {
    keys.onKeyDown(keyEvent(key));
  });
}

describe("usePaletteKeys", () => {
  it("moves the highlight and clamps at both ends", () => {
    const { result } = setup();
    expect(result.current.activeIndex).toBe(0);
    press(result.current, "ArrowDown");
    expect(result.current.activeIndex).toBe(1);
    press(result.current, "ArrowDown");
    press(result.current, "ArrowDown");
    expect(result.current.activeIndex).toBe(2);
    press(result.current, "ArrowUp");
    expect(result.current.activeIndex).toBe(1);
    press(result.current, "ArrowUp");
    press(result.current, "ArrowUp");
    expect(result.current.activeIndex).toBe(0);
  });

  it("activates the highlighted row on Enter", () => {
    const { result, onActivate } = setup();
    press(result.current, "ArrowDown");
    press(result.current, "Enter");
    expect(onActivate).toHaveBeenCalledOnce();
    expect(onActivate).toHaveBeenCalledWith(1);
  });

  it("moves the highlight directly on hover", () => {
    const { result } = setup();
    act(() => {
      result.current.setActiveIndex(2);
    });
    expect(result.current.activeIndex).toBe(2);
  });

  it("resets the highlight when the query changes", () => {
    const { result, rerender, onActivate, onClearQuery, onDismiss } = setup();
    press(result.current, "ArrowDown");
    expect(result.current.activeIndex).toBe(1);
    rerender({ count: 3, query: "a", onActivate, onClearQuery, onDismiss });
    expect(result.current.activeIndex).toBe(0);
  });

  it("clears first on Escape, then dismisses on the empty query", () => {
    const { result, onClearQuery, onDismiss } = setup({ query: "ab" });
    const event = keyEvent("Escape");
    act(() => {
      result.current.onKeyDown(event);
    });
    expect(onClearQuery).toHaveBeenCalledOnce();
    expect(onDismiss).not.toHaveBeenCalled();
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(event.stopPropagation).toHaveBeenCalledOnce();
  });

  it("dismisses on Escape with an empty query", () => {
    const { result, onClearQuery, onDismiss } = setup({ query: "" });
    press(result.current, "Escape");
    expect(onDismiss).toHaveBeenCalledOnce();
    expect(onClearQuery).not.toHaveBeenCalled();
  });

  it("steps back on Backspace with an empty query", () => {
    const onStepBack = vi.fn();
    const { result } = setup({ query: "", onStepBack });
    const event = keyEvent("Backspace");
    act(() => {
      result.current.onKeyDown(event);
    });
    expect(onStepBack).toHaveBeenCalledOnce();
    expect(event.preventDefault).toHaveBeenCalledOnce();
  });

  it("leaves Backspace alone with a query or nowhere to step", () => {
    const onStepBack = vi.fn();
    const filled = setup({ query: "ab", onStepBack });
    const event = keyEvent("Backspace");
    act(() => {
      filled.result.current.onKeyDown(event);
    });
    expect(onStepBack).not.toHaveBeenCalled();
    expect(event.preventDefault).not.toHaveBeenCalled();
    const root = setup({ query: "" });
    press(root.result.current, "Backspace");
    expect(root.onDismiss).not.toHaveBeenCalled();
  });

  it("ignores other keys", () => {
    const { result, onActivate, onDismiss } = setup();
    press(result.current, "a");
    expect(onActivate).not.toHaveBeenCalled();
    expect(onDismiss).not.toHaveBeenCalled();
    expect(result.current.activeIndex).toBe(0);
  });

  it("lets a focused button activate natively on Enter", () => {
    const { result, onActivate } = setup();
    const button = document.createElement("button");
    const event = { key: "Enter", target: button, preventDefault: vi.fn(), stopPropagation: vi.fn() };
    act(() => {
      result.current.onKeyDown(event as unknown as React.KeyboardEvent);
    });
    expect(onActivate).not.toHaveBeenCalled();
    expect(event.preventDefault).not.toHaveBeenCalled();
  });

  it("disables move and activate with zero rows", () => {
    const { result, onActivate } = setup({ count: 0 });
    expect(result.current.activeIndex).toBe(-1);
    press(result.current, "ArrowDown");
    press(result.current, "ArrowUp");
    press(result.current, "Enter");
    expect(onActivate).not.toHaveBeenCalled();
    expect(result.current.activeIndex).toBe(-1);
  });
});
