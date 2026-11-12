// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ARCHIVES_BACK_BREAKPOINT_PX, useArchivesBackVisible } from "./narrow.js";

function Probe(): React.ReactElement {
  return <p>{useArchivesBackVisible() ? "narrow" : "wide"}</p>;
}

interface MediaList {
  matches: boolean;
  listeners: Array<(event: { matches: boolean }) => void>;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("useArchivesBackVisible", () => {
  it("reads wide without matchMedia", () => {
    vi.stubGlobal("matchMedia", undefined);
    render(<Probe />);
    expect(screen.getByText("wide")).not.toBeNull();
  });

  it("matches at or below 640px and follows changes", () => {
    const list: MediaList = { matches: false, listeners: [] };
    vi.stubGlobal("matchMedia", (query: string) => {
      expect(query).toBe(`(max-width: ${ARCHIVES_BACK_BREAKPOINT_PX}px)`);
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
    expect(ARCHIVES_BACK_BREAKPOINT_PX).toBe(640);
    expect(screen.getByText("wide")).not.toBeNull();
    act(() => {
      list.matches = true;
      for (const listener of list.listeners) listener({ matches: true });
    });
    expect(screen.getByText("narrow")).not.toBeNull();
    unmount();
    expect(list.listeners).toHaveLength(0);
  });
});
