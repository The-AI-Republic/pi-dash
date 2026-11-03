// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { act } from "react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  Link: (props: { to?: string; children?: React.ReactNode; [key: string]: unknown }) => {
    const { to, children, ...rest } = props;
    return (
      <a href={to} {...rest}>
        {children}
      </a>
    );
  },
}));

import { selectProjectTabs, type ProjectTab } from "../tabs.js";
import { TabBar } from "./TabBar.js";

const TABS: ProjectTab[] = selectProjectTabs({
  workspaceSlug: "acme",
  projectId: "p1",
  flags: { cycle_view: true, module_view: true, issue_views_view: true, page_view: true, inbox_view: true },
  role: 20,
});

type ResizeEntry = { contentRect: { width: number } };

let resizeCallback: ((entries: ResizeEntry[]) => void) | null = null;
let disconnectCalls = 0;

class FakeResizeObserver {
  constructor(callback: (entries: ResizeEntry[]) => void) {
    resizeCallback = callback;
  }
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {
    disconnectCalls += 1;
  }
}

function stubObserver(): void {
  resizeCallback = null;
  disconnectCalls = 0;
  vi.stubGlobal("ResizeObserver", FakeResizeObserver);
}

function setup(
  overrides: Partial<{
    activeKey: string | null;
    defaultTab: string;
    hiddenTabs: string[];
    onToggleDefault: (key: string) => void;
    onHideTab: (key: string) => void;
    onShowTab: (key: string) => void;
  }> = {}
) {
  const handlers = {
    onToggleDefault: vi.fn(),
    onHideTab: vi.fn(),
    onShowTab: vi.fn(),
    ...overrides,
  };
  const view = render(
    <TabBar
      tabs={TABS}
      activeKey={overrides.activeKey ?? "work_items"}
      defaultTab={overrides.defaultTab ?? "work_items"}
      hiddenTabs={overrides.hiddenTabs ?? []}
      onToggleDefault={handlers.onToggleDefault as (key: string) => void}
      onHideTab={handlers.onHideTab as (key: string) => void}
      onShowTab={handlers.onShowTab as (key: string) => void}
    />
  );
  return { ...handlers, ...view };
}

/** Pin measured widths onto the rendered tab cells, in candidate order. */
function setCellWidths(widths: number[]): void {
  const cells = document.querySelectorAll("div.relative.flex.shrink-0");
  widths.forEach((width, index) => {
    const cell = cells[index];
    if (cell) Object.defineProperty(cell, "offsetWidth", { value: width, configurable: true });
  });
}

function fireResize(width: number): void {
  act(() => {
    resizeCallback?.([{ contentRect: { width } }]);
  });
}

function visibleTabNames(): string[] {
  return screen
    .queryAllByRole("link")
    .map((link) => link.textContent ?? "")
    .filter((name) => name.length > 0);
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("TabBar", () => {
  it("renders every tab with its address and marks the active one", () => {
    setup({ activeKey: "cycles" });
    const links = screen.getAllByRole("link");
    expect(links.map((link) => link.getAttribute("href"))).toEqual([
      "/acme/projects/p1/issues",
      "/acme/projects/p1/cycles",
      "/acme/projects/p1/modules",
      "/acme/projects/p1/views",
      "/acme/projects/p1/pages",
      "/acme/projects/p1/intake",
      "/acme/projects/p1/schedulers",
    ]);
    expect(screen.getByRole("link", { name: "Cycles" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: "Pages" })).not.toHaveAttribute("aria-current");
    expect(screen.queryByTestId("tab-overflow-trigger")).not.toBeInTheDocument();
  });

  it("toggles the default from the tab context menu", () => {
    const onToggleDefault = vi.fn();
    setup({ defaultTab: "cycles", onToggleDefault });
    fireEvent.contextMenu(screen.getByRole("link", { name: "Cycles" }));
    expect(screen.getByRole("menuitem", { name: "Clear default" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "Clear default" }));
    expect(onToggleDefault).toHaveBeenCalledWith("cycles");

    fireEvent.contextMenu(screen.getByRole("link", { name: "Pages" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Set as default" }));
    expect(onToggleDefault).toHaveBeenCalledWith("pages");
  });

  it("hides a tab from the tab context menu", () => {
    const onHideTab = vi.fn();
    setup({ onHideTab });
    fireEvent.contextMenu(screen.getByRole("link", { name: "Pages" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Hide in more menu" }));
    expect(onHideTab).toHaveBeenCalledWith("pages");
  });

  it("pools hidden tabs behind overflow with restore and default controls", async () => {
    const onShowTab = vi.fn();
    const onToggleDefault = vi.fn();
    setup({ hiddenTabs: ["pages"], defaultTab: "cycles", onShowTab, onToggleDefault });
    expect(visibleTabNames()).not.toContain("Pages");
    fireEvent.click(screen.getByTestId("tab-overflow-trigger"));
    const menu = await screen.findByTestId("tab-overflow-menu");
    expect(within(menu).getByRole("link", { name: "Pages" })).toHaveAttribute("href", "/acme/projects/p1/pages");
    fireEvent.click(within(menu).getByTitle("Show"));
    expect(onShowTab).toHaveBeenCalledWith("pages");
    fireEvent.click(within(menu).getByTitle("Set as default"));
    expect(onToggleDefault).toHaveBeenCalledWith("pages");
  });

  it("marks the default tab inside overflow rows", async () => {
    setup({ hiddenTabs: ["cycles"], defaultTab: "cycles" });
    fireEvent.click(screen.getByTestId("tab-overflow-trigger"));
    const menu = await screen.findByTestId("tab-overflow-menu");
    expect(within(menu).getByTitle("Clear default")).toBeInTheDocument();
  });

  it("pools trailing tabs when narrowed and pins the active tab", async () => {
    stubObserver();
    setup({ activeKey: "pages" });
    setCellWidths([100, 100, 100, 100, 100, 100, 100]);
    fireResize(360);
    await waitFor(() => expect(visibleTabNames()).toEqual(["Work Items", "Cycles", "Pages"]));
    expect(screen.getByRole("link", { name: "Pages" })).toHaveAttribute("aria-current", "page");
    fireEvent.click(screen.getByTestId("tab-overflow-trigger"));
    const menu = await screen.findByTestId("tab-overflow-menu");
    expect(
      within(menu)
        .getAllByRole("link")
        .map((link) => link.textContent)
    ).toEqual(["Modules", "Views", "Intake", "Schedulers"]);
    // Space-pooled rows carry no restore control.
    expect(within(menu).queryByTitle("Show")).not.toBeInTheDocument();
  });

  it("pools everything at zero width with the active tab marked", async () => {
    stubObserver();
    setup({ activeKey: "cycles" });
    setCellWidths([100, 100, 100, 100, 100, 100, 100]);
    fireResize(10);
    await waitFor(() => expect(screen.queryByTestId("tab-overflow-trigger")).toBeInTheDocument());
    expect(visibleTabNames()).toEqual([]);
    fireEvent.click(screen.getByTestId("tab-overflow-trigger"));
    const menu = await screen.findByTestId("tab-overflow-menu");
    expect(within(menu).getAllByRole("link")).toHaveLength(7);
    expect(within(menu).getByRole("link", { name: "Cycles" })).toHaveAttribute("aria-current", "page");
  });

  it("absorbs late-arriving tabs without losing the count", async () => {
    stubObserver();
    const handlers = { onToggleDefault: vi.fn(), onHideTab: vi.fn(), onShowTab: vi.fn() };
    const view = render(
      <TabBar
        tabs={TABS}
        activeKey="work_items"
        defaultTab="work_items"
        hiddenTabs={[]}
        onToggleDefault={handlers.onToggleDefault}
        onHideTab={handlers.onHideTab}
        onShowTab={handlers.onShowTab}
      />
    );
    setCellWidths([100, 100, 100, 100, 100, 100, 100]);
    fireResize(360);
    await waitFor(() => expect(visibleTabNames()).toEqual(["Work Items", "Cycles", "Modules"]));
    const extra: ProjectTab = {
      key: "reports",
      name: "Reports",
      segment: "reports",
      order: 8,
      flag: null,
      memberPlus: false,
      href: "/acme/projects/p1/reports",
    };
    view.rerender(
      <TabBar
        tabs={[...TABS, extra]}
        activeKey="work_items"
        defaultTab="work_items"
        hiddenTabs={[]}
        onToggleDefault={handlers.onToggleDefault}
        onHideTab={handlers.onHideTab}
        onShowTab={handlers.onShowTab}
      />
    );
    await waitFor(() => expect(visibleTabNames()).toEqual(["Work Items", "Cycles", "Modules"]));
    fireEvent.click(screen.getByTestId("tab-overflow-trigger"));
    const menu = await screen.findByTestId("tab-overflow-menu");
    expect(within(menu).getByRole("link", { name: "Reports" })).toBeInTheDocument();
  });

  it("ignores empty observer batches and disconnects on unmount", () => {
    stubObserver();
    const view = setup();
    act(() => {
      resizeCallback?.([]);
    });
    expect(visibleTabNames()).toHaveLength(7);
    view.unmount();
    expect(disconnectCalls).toBe(1);
  });
});
