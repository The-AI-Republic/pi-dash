// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  SIDEBAR_DEFAULT_WIDTH,
  SIDEBAR_MAX_WIDTH,
  SIDEBAR_MIN_WIDTH,
  sidebarPanelStorageKey,
  useShellChrome,
} from "./chromeState.js";
import { SIDEBAR_PEEK_DISMISS_MS, SidebarShell } from "./SidebarShell.js";

let narrowMatches = false;
let changeListeners: Array<(event: { matches: boolean }) => void> = [];

function stubViewport(): void {
  vi.stubGlobal("matchMedia", () => ({
    get matches() {
      return narrowMatches;
    },
    addEventListener: (_type: string, listener: (event: { matches: boolean }) => void) => {
      changeListeners.push(listener);
    },
    removeEventListener: (_type: string, listener: (event: { matches: boolean }) => void) => {
      changeListeners = changeListeners.filter((entry) => entry !== listener);
    },
  }));
}

function setNarrow(next: boolean): void {
  narrowMatches = next;
  act(() => {
    for (const listener of changeListeners) listener({ matches: next });
  });
}

function resetMain(): void {
  useShellChrome.setState({
    sidebarCollapsed: false,
    peekOpen: false,
    sidebarWidth: SIDEBAR_DEFAULT_WIDTH,
  });
  globalThis.localStorage.removeItem(sidebarPanelStorageKey("main"));
}

function shell(props: Partial<React.ComponentProps<typeof SidebarShell>> = {}): React.ReactElement {
  return (
    <>
      <SidebarShell
        brand={<span data-testid="brand">Pi Dash</span>}
        onPersonalize={() => undefined}
        quickActions={<button type="button">Drill</button>}
        account={<button type="button">Account</button>}
        {...props}
      >
        <p>Scroll content</p>
      </SidebarShell>
      <main>
        <p>Page body</p>
      </main>
    </>
  );
}

beforeEach(() => {
  narrowMatches = false;
  changeListeners = [];
  stubViewport();
  resetMain();
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
  resetMain();
});

describe("SidebarShell", () => {
  it("renders header, quick actions, scroll content and account while open", () => {
    const onPersonalize = vi.fn();
    render(shell({ onPersonalize }));
    expect(screen.getByTestId("brand")).not.toBeNull();
    fireEvent.click(screen.getByTestId("personalize-nav"));
    expect(onPersonalize).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("sidebar-quick-actions")).not.toBeNull();
    expect(screen.getByText("Scroll content")).not.toBeNull();
    expect(screen.getByRole("button", { name: "Account" })).not.toBeNull();
    const aside = screen.getByRole("complementary", { name: "Workspace navigation" });
    expect(aside.getAttribute("style")).toContain(`width: ${SIDEBAR_DEFAULT_WIDTH}px`);
    expect(aside).not.toHaveAttribute("inert");
    expect(aside.className).toContain("transition-[width]");
  });

  it("omits the launcher, quick actions and account slots when unprovided", () => {
    render(shell({ onPersonalize: undefined, quickActions: undefined, account: undefined }));
    expect(screen.queryByTestId("personalize-nav")).toBeNull();
    expect(screen.queryByTestId("sidebar-quick-actions")).toBeNull();
    expect(screen.queryByRole("button", { name: "Account" })).toBeNull();
  });

  it("collapses to an inert zero-width strip with the peek trigger and no account", () => {
    render(shell());
    act(() => {
      useShellChrome.getState().setSidebarCollapsed(true);
    });
    // Inert landmarks leave the accessibility tree; locate by id instead.
    const aside = document.getElementById("main-sidebar");
    expect(aside?.getAttribute("style")).toContain("width: 0px");
    expect(aside?.className).toContain("w-0");
    expect(aside).toHaveAttribute("inert");
    expect(screen.queryByRole("button", { name: "Account" })).toBeNull();
    expect(screen.getByTestId("sidebar-peek-trigger")).not.toBeNull();
    expect(screen.queryByTestId("sidebar-peek")).toBeNull();
  });

  it("drags the grip to resize, clamped and cursor-flagged, until release", () => {
    render(shell());
    const grip = screen.getByRole("separator", { name: "Resize sidebar" });
    fireEvent.mouseDown(grip, { clientX: 256 });
    expect(document.body.style.cursor).toBe("col-resize");
    expect(document.body.style.userSelect).toBe("none");
    expect(grip.className).toContain("bg-(--accent)");
    fireEvent.mouseMove(document, { clientX: 256 + 44 });
    expect(useShellChrome.getState().sidebarWidth).toBe(300);
    fireEvent.mouseMove(document, { clientX: 256 + 4000 });
    expect(useShellChrome.getState().sidebarWidth).toBe(SIDEBAR_MAX_WIDTH);
    fireEvent.mouseMove(document, { clientX: 256 - 4000 });
    expect(useShellChrome.getState().sidebarWidth).toBe(SIDEBAR_MIN_WIDTH);
    fireEvent.mouseUp(document);
    expect(document.body.style.cursor).toBe("");
    expect(document.body.style.userSelect).toBe("");
    expect(screen.getByRole("separator", { name: "Resize sidebar" }).className).not.toContain("bg-(--accent)");
    fireEvent.mouseMove(document, { clientX: 256 });
    expect(useShellChrome.getState().sidebarWidth).toBe(SIDEBAR_MIN_WIDTH);
  });

  it("double-clicking the grip flips collapse and clears peek", () => {
    render(shell());
    act(() => {
      useShellChrome.getState().setPeekOpen(true);
    });
    fireEvent.doubleClick(screen.getByRole("separator", { name: "Resize sidebar" }));
    expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
    expect(useShellChrome.getState().peekOpen).toBe(false);
  });

  it("hovers the collapsed edge open into a content-only overlay", () => {
    render(shell());
    act(() => {
      useShellChrome.getState().setSidebarCollapsed(true);
    });
    fireEvent.pointerEnter(screen.getByTestId("sidebar-peek-trigger"));
    const overlay = screen.getByTestId("sidebar-peek");
    expect(overlay.getAttribute("style")).toContain(`width: ${SIDEBAR_DEFAULT_WIDTH}px`);
    // The overlay previews navigation, not actions: no launcher, no
    // quick actions, and no second account menu.
    expect(overlay.textContent).toContain("Scroll content");
    expect(overlay.querySelector('[data-testid="personalize-nav"]')).toBeNull();
    expect(overlay.querySelector('[data-testid="sidebar-quick-actions"]')).toBeNull();
    expect(overlay.textContent).not.toContain("Account");
  });

  it("opens peek through trigger click and focus too", () => {
    render(shell());
    act(() => {
      useShellChrome.getState().setSidebarCollapsed(true);
    });
    fireEvent.click(screen.getByTestId("sidebar-peek-trigger"));
    expect(screen.queryByTestId("sidebar-peek")).not.toBeNull();
    act(() => {
      useShellChrome.getState().clearPeek();
    });
    fireEvent.focus(screen.getByTestId("sidebar-peek-trigger"));
    expect(screen.queryByTestId("sidebar-peek")).not.toBeNull();
  });

  it("dismisses the vacated overlay after its linger, cancelled on return", () => {
    vi.useFakeTimers();
    render(shell());
    act(() => {
      useShellChrome.getState().setSidebarCollapsed(true);
      useShellChrome.getState().setPeekOpen(true);
    });
    const overlay = screen.getByTestId("sidebar-peek");
    fireEvent.pointerLeave(overlay);
    fireEvent.pointerEnter(overlay);
    act(() => {
      vi.advanceTimersByTime(SIDEBAR_PEEK_DISMISS_MS);
    });
    expect(screen.queryByTestId("sidebar-peek")).not.toBeNull();
    // A second leave replaces the first timer instead of stacking one.
    fireEvent.pointerLeave(overlay);
    fireEvent.pointerLeave(overlay);
    act(() => {
      vi.advanceTimersByTime(SIDEBAR_PEEK_DISMISS_MS - 1);
    });
    expect(screen.queryByTestId("sidebar-peek")).not.toBeNull();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.queryByTestId("sidebar-peek")).toBeNull();
  });

  it("closes peek on Escape but ignores other keys", () => {
    render(shell());
    act(() => {
      useShellChrome.getState().setSidebarCollapsed(true);
      useShellChrome.getState().setPeekOpen(true);
    });
    const overlay = screen.getByTestId("sidebar-peek");
    fireEvent.keyDown(overlay, { key: "a" });
    expect(screen.queryByTestId("sidebar-peek")).not.toBeNull();
    fireEvent.keyDown(overlay, { key: "Escape" });
    expect(screen.queryByTestId("sidebar-peek")).toBeNull();
  });

  it("keeps peek when trigger focus moves into the overlay, closes it otherwise", () => {
    render(shell());
    act(() => {
      useShellChrome.getState().setSidebarCollapsed(true);
    });
    const trigger = screen.getByTestId("sidebar-peek-trigger");
    fireEvent.focus(trigger);
    const overlay = screen.getByTestId("sidebar-peek");
    fireEvent.blur(trigger, { relatedTarget: overlay });
    expect(screen.queryByTestId("sidebar-peek")).not.toBeNull();
    fireEvent.blur(trigger, { relatedTarget: document.body });
    expect(screen.queryByTestId("sidebar-peek")).toBeNull();
    // Blur with the overlay already gone still settles closed.
    fireEvent.focus(trigger);
    fireEvent.keyDown(screen.getByTestId("sidebar-peek"), { key: "Escape" });
    fireEvent.blur(trigger, { relatedTarget: document.body });
    expect(useShellChrome.getState().peekOpen).toBe(false);
  });

  it("never fires a pending dismissal after unmount", () => {
    vi.useFakeTimers();
    const { unmount } = render(shell());
    act(() => {
      useShellChrome.getState().setSidebarCollapsed(true);
      useShellChrome.getState().setPeekOpen(true);
    });
    fireEvent.pointerLeave(screen.getByTestId("sidebar-peek"));
    unmount();
    act(() => {
      vi.advanceTimersByTime(SIDEBAR_PEEK_DISMISS_MS * 2);
    });
    expect(useShellChrome.getState().peekOpen).toBe(true);
  });

  it("auto-collapses crossing below the breakpoint and clears peek", () => {
    render(shell());
    act(() => {
      useShellChrome.getState().setPeekOpen(true);
    });
    setNarrow(true);
    expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
    expect(useShellChrome.getState().peekOpen).toBe(false);
    // Re-crossing while already collapsed is a no-op, not a toggle.
    setNarrow(false);
    setNarrow(true);
    expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
  });

  it("starts open and floating mounting narrow, and toggling while narrow sticks", () => {
    // Narrow loads show the sidebar (SHELL-062's autohide scenario taps
    // a link to dismiss it); only crossing below the breakpoint
    // collapses. This supersedes the earlier mount-collapsed reading.
    narrowMatches = true;
    render(shell());
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
    const aside = screen.getByRole("complementary", { name: "Workspace navigation" });
    expect(aside.getAttribute("style")).toContain(`width: ${SIDEBAR_DEFAULT_WIDTH}px`);
    expect(screen.getByRole("button", { name: "Account" })).not.toBeNull();
    // The open sidebar floats above content; no peek trigger mounts.
    expect(aside.className).toContain("absolute");
    expect(aside.className).toContain("z-40");
    expect(aside.className).toContain("shadow-popover");
    expect(screen.queryByTestId("sidebar-peek-trigger")).toBeNull();
    act(() => {
      useShellChrome.getState().toggleSidebarCollapsed();
    });
    expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
    act(() => {
      useShellChrome.getState().toggleSidebarCollapsed();
    });
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
  });

  it("collapses the floating sidebar on outside presses only", () => {
    narrowMatches = true;
    render(shell());
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
    fireEvent.pointerDown(screen.getByText("Scroll content"));
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
    fireEvent.pointerDown(screen.getByText("Page body"));
    expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
    // Presses while narrow and collapsed fall through: no listener rides.
    fireEvent.pointerDown(screen.getByText("Page body"));
    expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
    // Widening never reopens: the toggle does.
    setNarrow(false);
    expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
  });

  it("collapses the floating sidebar on content taps (SHELL-062)", () => {
    narrowMatches = true;
    render(
      <SidebarShell brand={<span>Pi Dash</span>}>
        <a href="/acme/">Home</a>
        <button type="button">Project</button>
        <button type="button" data-keep-open="true">
          Switcher
        </button>
      </SidebarShell>
    );
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
    // Same-address taps collapse too: the tap, not the navigation, hides.
    fireEvent.click(screen.getByRole("link", { name: "Home" }));
    expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
    act(() => {
      useShellChrome.getState().toggleSidebarCollapsed();
    });
    fireEvent.click(screen.getByRole("button", { name: "Project" }));
    expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
    act(() => {
      useShellChrome.getState().toggleSidebarCollapsed();
    });
    // Menu and dialog triggers opt out: opening them never collapses.
    fireEvent.click(screen.getByRole("button", { name: "Switcher" }));
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
  });

  it("ignores content taps while docked or collapsed", () => {
    render(
      <SidebarShell brand={<span>Pi Dash</span>}>
        <a href="/acme/">Home</a>
      </SidebarShell>
    );
    fireEvent.click(screen.getByRole("link", { name: "Home" }));
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
    act(() => {
      useShellChrome.getState().setSidebarCollapsed(true);
    });
    fireEvent.click(screen.getByText("Home"));
    expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
  });

  it("ignores outside presses while docked", () => {
    render(shell());
    fireEvent.pointerDown(screen.getByText("Page body"));
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
  });

  it("lets the top-bar toggle flip the floating sidebar closed", () => {
    narrowMatches = true;
    // Wired like the gated route's onToggleSidebar: peek clears, then
    // the click flips. The nested icon mirrors the real toggle's svg.
    render(
      <>
        <button
          type="button"
          data-testid="sidebar-toggle"
          onClick={() => {
            useShellChrome.getState().clearPeek();
            useShellChrome.getState().toggleSidebarCollapsed();
          }}
        >
          <span data-testid="sidebar-toggle-icon">Toggle</span>
        </button>
        {shell()}
      </>
    );
    // Narrow mounts open (SHELL-062); the pointerdown lands first,
    // outside the aside: it must fall through, or the click would flip
    // the sidebar back open.
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
    fireEvent.pointerDown(screen.getByTestId("sidebar-toggle-icon"));
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
    fireEvent.pointerDown(screen.getByTestId("sidebar-toggle"));
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
    fireEvent.click(screen.getByTestId("sidebar-toggle"));
    expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
  });

  it("drives named panels from independent snapshots", () => {
    const { unmount } = render(
      <>
        {shell()}
        {shell({ panel: "extended", domId: "extended-sidebar" })}
      </>
    );
    act(() => {
      useShellChrome.getState().setSidebarCollapsed(true);
    });
    expect(document.getElementById("main-sidebar")?.getAttribute("style")).toContain("width: 0px");
    const extended = document.getElementById("extended-sidebar");
    expect(extended?.getAttribute("style")).toContain(`width: ${SIDEBAR_DEFAULT_WIDTH}px`);
    unmount();
    globalThis.localStorage.removeItem(sidebarPanelStorageKey("extended"));
  });
});
