// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar UI state: the persisted collapse flag with its narrow
// default, and the per-cycle breakdown tab (CYC-035, CYC-039).

import { act, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CYCLE_SIDEBAR_COLLAPSED_KEY } from "../lib/cycleSidebar.js";
import { useCycleAnalyticsTab, useCycleSidebarCollapsed } from "./cycleSidebarState.js";

function stubViewport(narrow: boolean): void {
  vi.stubGlobal("matchMedia", () => ({
    matches: narrow,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
}

function memoryStorage(entries: Record<string, string> = {}): Storage {
  const map = new Map(Object.entries(entries));
  return {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => {
      map.set(key, value);
    },
    removeItem: (key: string) => {
      map.delete(key);
    },
  } as Storage;
}

afterEach(() => {
  vi.unstubAllGlobals();
  window.localStorage.clear();
});

describe("useCycleSidebarCollapsed", () => {
  function Probe({ storage }: { storage?: Storage | undefined }): React.ReactElement {
    const [collapsed, setCollapsed] = useCycleSidebarCollapsed(storage);
    return (
      <button
        type="button"
        data-testid="probe"
        data-collapsed={String(collapsed)}
        onClick={() => setCollapsed(!collapsed)}
      >
        {collapsed ? "collapsed" : "open"}
      </button>
    );
  }

  it("opens fresh wide viewports and persists the choice", () => {
    stubViewport(false);
    const storage = memoryStorage();
    render(<Probe storage={storage} />);
    expect(screen.getByTestId("probe")).toHaveTextContent("open");
    act(() => {
      screen.getByTestId("probe").click();
    });
    expect(screen.getByTestId("probe")).toHaveTextContent("collapsed");
    expect(storage.getItem(CYCLE_SIDEBAR_COLLAPSED_KEY)).toBe("true");
    act(() => {
      screen.getByTestId("probe").click();
    });
    expect(storage.getItem(CYCLE_SIDEBAR_COLLAPSED_KEY)).toBe(null);
  });

  it("starts fresh narrow viewports collapsed", () => {
    stubViewport(true);
    render(<Probe storage={memoryStorage()} />);
    expect(screen.getByTestId("probe")).toHaveTextContent("collapsed");
  });

  it("honors a stored collapse on any viewport", () => {
    stubViewport(false);
    render(<Probe storage={memoryStorage({ [CYCLE_SIDEBAR_COLLAPSED_KEY]: "true" })} />);
    expect(screen.getByTestId("probe")).toHaveTextContent("collapsed");
  });

  it("reads wide viewports without matchMedia", () => {
    vi.stubGlobal("matchMedia", undefined);
    render(<Probe storage={memoryStorage()} />);
    expect(screen.getByTestId("probe")).toHaveTextContent("open");
  });

  it("defaults to the browser profile storage", () => {
    stubViewport(false);
    window.localStorage.setItem(CYCLE_SIDEBAR_COLLAPSED_KEY, "true");
    render(<Probe />);
    expect(screen.getByTestId("probe")).toHaveTextContent("collapsed");
  });
});

describe("useCycleAnalyticsTab", () => {
  function Probe({ storage }: { storage?: Storage | undefined }): React.ReactElement {
    const [tab, setTab] = useCycleAnalyticsTab("cycle-1", storage);
    return (
      <button type="button" data-testid="probe" onClick={() => setTab("States")}>
        {tab}
      </button>
    );
  }

  it("defaults to Assignees and remembers the pick per cycle", () => {
    const storage = memoryStorage();
    render(<Probe storage={storage} />);
    expect(screen.getByTestId("probe")).toHaveTextContent("Assignees");
    act(() => {
      screen.getByTestId("probe").click();
    });
    expect(storage.getItem("cycle-analytics-tab-cycle-1")).toBe("States");
  });

  it("reads the stored tab", () => {
    render(<Probe storage={memoryStorage({ "cycle-analytics-tab-cycle-1": "Labels" })} />);
    expect(screen.getByTestId("probe")).toHaveTextContent("Labels");
  });

  it("defaults to the browser profile storage", () => {
    window.localStorage.setItem("cycle-analytics-tab-cycle-1", "States");
    render(<Probe />);
    expect(screen.getByTestId("probe")).toHaveTextContent("States");
  });
});
