// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  EDITOR_PORTAL_ID,
  FULL_SCREEN_PORTAL_ID,
  MENU_PORTAL_ID,
  OverlayPortal,
  markInitialColorScheme,
  useDocumentHead,
} from "./document.js";

function seedServedHead(): void {
  document.head.innerHTML = [
    '<meta name="description" content="served description">',
    '<meta property="og:title" content="served title">',
    '<meta name="twitter:card" content="summary">',
    '<meta name="viewport" content="width=device-width, initial-scale=1.0">',
    '<meta name="theme-color" content="#ffffff">',
  ].join("");
  document.title = "served title";
}

function mockColorScheme(dark: boolean): void {
  Object.defineProperty(globalThis.window, "matchMedia", {
    configurable: true,
    value: vi.fn(() => ({ matches: dark }) as MediaQueryList),
  });
}

function HeadProbe({
  title,
  robots,
  fallback = false,
  children,
}: {
  title: string;
  robots: string;
  fallback?: boolean;
  children?: React.ReactNode;
}): React.ReactElement {
  useDocumentHead({ title, robots, fallback });
  return <span>probe{children}</span>;
}

describe("useDocumentHead", () => {
  it("sets the title and robots, swapping the served metadata out", () => {
    seedServedHead();
    render(<HeadProbe title="Workspace home" robots="index, nofollow" />);

    expect(document.title).toBe("Workspace home");
    expect(document.querySelector('meta[name="robots"]')?.getAttribute("content")).toBe("index, nofollow");
    expect(document.querySelector('meta[name="description"]')).toBeNull();
    expect(document.querySelector('meta[property="og:title"]')).toBeNull();
    expect(document.querySelector('meta[name="twitter:card"]')).toBeNull();
    // Static markers stay put.
    expect(document.querySelector('meta[name="viewport"]')).not.toBeNull();
    expect(document.querySelector('meta[name="theme-color"]')).not.toBeNull();
  });

  it("restores the previous title and robots on unmount", () => {
    document.title = "previous";
    const { unmount } = render(<HeadProbe title="Ephemeral" robots="noindex, nofollow" />);
    expect(document.title).toBe("Ephemeral");

    unmount();
    expect(document.title).toBe("previous");
    expect(document.querySelector('meta[name="robots"]')).toBeNull();
  });

  it("restores a content-less robots tag to content-less", () => {
    document.head.innerHTML = '<meta name="robots">';
    const { unmount } = render(<HeadProbe title="Ephemeral" robots="noindex, nofollow" />);
    expect(document.querySelector('meta[name="robots"]')?.getAttribute("content")).toBe("noindex, nofollow");

    unmount();
    const restored = document.querySelector('meta[name="robots"]');
    expect(restored).not.toBeNull();
    expect(restored?.hasAttribute("content")).toBe(false);
  });

  it("reuses an existing robots tag and restores its content", () => {
    document.head.innerHTML = '<meta name="robots" content="index, nofollow">';
    const { unmount } = render(<HeadProbe title="Ephemeral" robots="noindex, nofollow" />);
    expect(document.querySelector('meta[name="robots"]')?.getAttribute("content")).toBe("noindex, nofollow");

    unmount();
    expect(document.querySelector('meta[name="robots"]')?.getAttribute("content")).toBe("index, nofollow");
  });

  it("prefers a nested route title over the root fallback despite effect order", () => {
    // Parent effects run after child effects, so the fallback writes
    // last — the route must still win (gate-found home title clobber).
    document.title = "served title";
    render(
      <HeadProbe title="Pi Dash" robots="index, nofollow" fallback>
        <HeadProbe title="Parity Workspace · Home" robots="noindex, nofollow" />
      </HeadProbe>
    );

    expect(document.title).toBe("Parity Workspace · Home");
    expect(document.querySelector('meta[name="robots"]')?.getAttribute("content")).toBe("noindex, nofollow");
  });

  it("restores the surviving fallback when the route unmounts", () => {
    document.title = "served title";
    const { rerender } = render(
      <HeadProbe title="Pi Dash" robots="index, nofollow" fallback>
        <HeadProbe title="Parity Workspace · Home" robots="noindex, nofollow" />
      </HeadProbe>
    );
    expect(document.title).toBe("Parity Workspace · Home");

    rerender(
      <HeadProbe title="Pi Dash" robots="index, nofollow" fallback>
        <span>route gone</span>
      </HeadProbe>
    );
    expect(document.title).toBe("Pi Dash");
    expect(document.querySelector('meta[name="robots"]')?.getAttribute("content")).toBe("index, nofollow");
  });

  it("prefers the newest route when two routes overlap, then reveals the older", () => {
    document.title = "served title";
    const { rerender } = render(
      <HeadProbe title="Pi Dash" robots="index, nofollow" fallback>
        <HeadProbe title="First route" robots="noindex, nofollow" />
      </HeadProbe>
    );
    rerender(
      <HeadProbe title="Pi Dash" robots="index, nofollow" fallback>
        <HeadProbe title="First route" robots="noindex, nofollow" />
        <HeadProbe title="Second route" robots="noindex, nofollow" />
      </HeadProbe>
    );
    expect(document.title).toBe("Second route");

    rerender(
      <HeadProbe title="Pi Dash" robots="index, nofollow" fallback>
        <HeadProbe title="First route" robots="noindex, nofollow" />
      </HeadProbe>
    );
    expect(document.title).toBe("First route");
  });

  it("reapplies a route title when it changes", () => {
    document.title = "served title";
    const { rerender } = render(
      <HeadProbe title="Pi Dash" robots="index, nofollow" fallback>
        <HeadProbe title="workspace-slug · Home" robots="noindex, nofollow" />
      </HeadProbe>
    );
    expect(document.title).toBe("workspace-slug · Home");

    rerender(
      <HeadProbe title="Pi Dash" robots="index, nofollow" fallback>
        <HeadProbe title="Workspace Name · Home" robots="noindex, nofollow" />
      </HeadProbe>
    );
    expect(document.title).toBe("Workspace Name · Home");
  });
});

describe("markInitialColorScheme", () => {
  it("marks a light root when the OS prefers light", () => {
    mockColorScheme(false);
    expect(markInitialColorScheme()).toBe("light");
    expect(document.documentElement.style.getPropertyValue("color-scheme")).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("marks a dark root when the OS prefers dark", () => {
    mockColorScheme(true);
    expect(markInitialColorScheme()).toBe("dark");
    expect(document.documentElement.style.getPropertyValue("color-scheme")).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });
});

describe("OverlayPortal", () => {
  it("renders children into the named static root", () => {
    const host = document.createElement("div");
    host.id = MENU_PORTAL_ID;
    document.body.appendChild(host);

    render(
      <OverlayPortal rootId={MENU_PORTAL_ID}>
        <button type="button">menu item</button>
      </OverlayPortal>
    );
    expect(host.querySelector("button")).not.toBeNull();
    expect(screen.getByRole("button", { name: "menu item" })).not.toBeNull();
    host.remove();
  });

  it("renders nothing when the static root is absent", () => {
    document.getElementById(EDITOR_PORTAL_ID)?.remove();
    document.getElementById(FULL_SCREEN_PORTAL_ID)?.remove();
    const { container } = render(
      <OverlayPortal rootId={FULL_SCREEN_PORTAL_ID}>
        <span>overlay</span>
      </OverlayPortal>
    );
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByText("overlay")).toBeNull();
  });
});
