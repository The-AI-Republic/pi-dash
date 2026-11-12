// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useTouchInput } from "./touchInput.js";

function Probe(): React.ReactElement {
  return <p>{useTouchInput() ? "touch" : "pointer"}</p>;
}

interface MediaList {
  matches: boolean;
  listeners: Array<() => void>;
}

function coarsePointer(initial: boolean): MediaList {
  const list: MediaList = { matches: initial, listeners: [] };
  vi.stubGlobal("matchMedia", (query: string) => {
    expect(query).toBe("(pointer: coarse)");
    return {
      get matches() {
        return list.matches;
      },
      addEventListener: (_type: string, listener: () => void) => {
        list.listeners.push(listener);
      },
      removeEventListener: (_type: string, listener: () => void) => {
        list.listeners = list.listeners.filter((entry) => entry !== listener);
      },
    };
  });
  return list;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("useTouchInput", () => {
  it("reads a coarse primary pointer", () => {
    coarsePointer(true);
    render(<Probe />);
    expect(screen.getByText("touch")).not.toBeNull();
  });

  it("reads a fine pointer on a touch-capable device", () => {
    coarsePointer(false);
    vi.stubGlobal("navigator", { maxTouchPoints: 5 });
    render(<Probe />);
    expect(screen.getByText("touch")).not.toBeNull();
  });

  it("reads a plain mouse device as pointer input and follows changes", () => {
    const list = coarsePointer(false);
    vi.stubGlobal("navigator", { maxTouchPoints: 0 });
    const { unmount } = render(<Probe />);
    expect(screen.getByText("pointer")).not.toBeNull();
    act(() => {
      list.matches = true;
      for (const listener of list.listeners) listener();
    });
    expect(screen.getByText("touch")).not.toBeNull();
    unmount();
    expect(list.listeners).toHaveLength(0);
  });

  it("reads pointer input without matchMedia or navigator", () => {
    vi.stubGlobal("matchMedia", undefined);
    vi.stubGlobal("navigator", undefined);
    render(<Probe />);
    expect(screen.getByText("pointer")).not.toBeNull();
  });
});
