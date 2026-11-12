// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { NARROW_VIEWPORT_BREAKPOINT_PX, useNarrowViewport } from "./narrowViewport.js";

function Probe({ breakpoint }: { breakpoint?: number }): React.ReactElement {
  return <p>{useNarrowViewport(breakpoint) ? "narrow" : "wide"}</p>;
}

interface MediaList {
  matches: boolean;
  listeners: Array<(event: { matches: boolean }) => void>;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("useNarrowViewport", () => {
  it("reads wide without matchMedia", () => {
    vi.stubGlobal("matchMedia", undefined);
    render(<Probe />);
    expect(screen.getByText("wide")).not.toBeNull();
  });

  it("reads the initial match and follows changes", () => {
    // One list per query, like the browser: the initializer and the
    // effect both call matchMedia and must observe the same object.
    const list: MediaList = { matches: false, listeners: [] };
    vi.stubGlobal("matchMedia", (query: string) => {
      expect(query).toBe(`(max-width: ${NARROW_VIEWPORT_BREAKPOINT_PX - 1}px)`);
      return {
        get matches() {
          return list.matches;
        },
        addEventListener: (_type: string, listener: (event: { matches: boolean }) => void) => {
          list.listeners.push(listener);
        },
        removeEventListener: (_type: string, listener: (event: { matches: boolean }) => void) => {
          list.listeners = list.listeners.filter((entry) => entry !== listener);
        },
      };
    });
    const { unmount } = render(<Probe />);
    expect(screen.getByText("wide")).not.toBeNull();
    act(() => {
      list.matches = true;
      for (const listener of list.listeners) listener({ matches: true });
    });
    expect(screen.getByText("narrow")).not.toBeNull();
    unmount();
    expect(list.listeners).toHaveLength(0);
  });

  it("honors a custom breakpoint", () => {
    vi.stubGlobal("matchMedia", (query: string) => {
      expect(query).toBe("(max-width: 499px)");
      return { matches: true, addEventListener: () => undefined, removeEventListener: () => undefined };
    });
    render(<Probe breakpoint={500} />);
    expect(screen.getByText("narrow")).not.toBeNull();
  });
});
