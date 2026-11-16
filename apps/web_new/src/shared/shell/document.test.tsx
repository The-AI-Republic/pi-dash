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
  type HeadTag,
} from "./document.js";

function seedServedHead(): void {
  document.head.innerHTML = [
    '<meta name="description" content="served description">',
    '<meta property="og:title" content="served title">',
    '<meta name="twitter:card" content="summary">',
    '<link rel="canonical" href="https://site.test/served">',
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

/** Canonical + description + og:url for one address, the way a page owner sends them. */
function CANONICAL_TAGS(path: string): HeadTag[] {
  const url = `https://site.test/${path}`;
  return [
    { tag: "link", attributes: { rel: "canonical", href: url } },
    { tag: "meta", attributes: { name: "description", content: `${path} description.` } },
    { tag: "meta", attributes: { property: "og:url", content: url } },
    { tag: "meta", attributes: { property: "og:image:width", content: "1200" } },
  ];
}

function HeadProbe({
  title,
  robots,
  fallback = false,
  tags,
  children,
}: {
  title: string;
  robots: string;
  fallback?: boolean;
  tags?: readonly HeadTag[];
  children?: React.ReactNode;
}): React.ReactElement {
  useDocumentHead({
    title,
    robots,
    fallback,
    ...(tags === undefined ? {} : { tags }),
  });
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

  it("writes canonical, description and social preview tags for a crawlable page", () => {
    seedServedHead();
    render(
      <HeadProbe
        title="Pi Dash docs"
        robots="index, follow"
        tags={[
          { tag: "link", attributes: { rel: "canonical", href: "https://site.test/docs" } },
          { tag: "meta", attributes: { name: "description", content: "How Pi Dash works." } },
          { tag: "meta", attributes: { property: "og:url", content: "https://site.test/docs" } },
          { tag: "meta", attributes: { property: "og:image:width", content: "1200" } },
        ]}
      />
    );
    expect(document.querySelector('link[rel="canonical"]')?.getAttribute("href")).toBe("https://site.test/docs");
    expect(document.querySelector('meta[property="og:url"]')?.getAttribute("content")).toBe("https://site.test/docs");
    expect(document.querySelector('meta[name="description"]')?.getAttribute("content")).toBe("How Pi Dash works.");
    expect(document.querySelector('meta[property="og:image:width"]')?.getAttribute("content")).toBe("1200");
    // The served canonical is replaced, not duplicated (CLOUD-005).
    expect(document.querySelectorAll('link[rel="canonical"]')).toHaveLength(1);
  });

  it("takes the managed tags away when a crawlable page unmounts", () => {
    document.title = "served";
    const { unmount } = render(<HeadProbe title="Pi Dash docs" robots="index, follow" tags={CANONICAL_TAGS("docs")} />);
    expect(document.querySelectorAll('link[rel="canonical"]')).toHaveLength(1);
    unmount();
    expect(document.querySelector('link[rel="canonical"]')).toBeNull();
    expect(document.querySelector('meta[property="og:url"]')).toBeNull();
    expect(document.querySelector('meta[name="description"]')).toBeNull();
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

  it("replaces managed tags left by an earlier route on a fresh mount", () => {
    document.head.innerHTML = "";
    const stale = document.createElement("meta");
    stale.setAttribute("property", "og:url");
    stale.setAttribute("content", "https://site.test/stale");
    stale.setAttribute("data-route-head", "");
    document.head.appendChild(stale);

    render(<HeadProbe title="Pi Dash docs" robots="index, follow" tags={CANONICAL_TAGS("docs")} />);
    const ogUrls = document.querySelectorAll('meta[property="og:url"]');
    expect(ogUrls).toHaveLength(1);
    expect(ogUrls[0]?.getAttribute("content")).toBe("https://site.test/docs");
  });

  it("replaces the managed tags when a crawlable page changes address", () => {
    seedServedHead();
    const { rerender } = render(
      <HeadProbe title="Pi Dash docs" robots="index, follow" tags={CANONICAL_TAGS("docs")} />
    );
    rerender(<HeadProbe title="Pi Dash pricing" robots="index, follow" tags={CANONICAL_TAGS("pricing")} />);
    const canonicals = document.querySelectorAll('link[rel="canonical"]');
    expect(canonicals).toHaveLength(1);
    expect(canonicals[0]?.getAttribute("href")).toBe("https://site.test/pricing");
    expect(document.querySelectorAll('meta[name="description"]')).toHaveLength(1);
    expect(document.querySelector('meta[property="og:url"]')?.getAttribute("content")).toBe(
      "https://site.test/pricing"
    );
  });
});

describe("document-less environments", () => {
  function withoutDocument(run: () => void): void {
    const realDocument = globalThis.document;
    // The prerender build imports this module in Node, where no DOM exists.
    Object.defineProperty(globalThis, "document", { configurable: true, get: () => undefined });
    try {
      run();
    } finally {
      Object.defineProperty(globalThis, "document", { configurable: true, value: realDocument });
    }
  }

  it("resolves the initial scheme without touching a document", () => {
    mockColorScheme(true);
    withoutDocument(() => expect(markInitialColorScheme()).toBe("dark"));
  });

  it("renders no portal without a document", () => {
    withoutDocument(() => {
      expect(OverlayPortal({ rootId: MENU_PORTAL_ID, children: <span>overlay</span> })).toBeNull();
    });
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

  it("keeps a stored light choice over an OS that prefers dark", () => {
    mockColorScheme(true);
    localStorage.setItem("theme", "light");
    expect(markInitialColorScheme()).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    localStorage.removeItem("theme");
  });

  it("keeps a stored dark choice over an OS that prefers light", () => {
    mockColorScheme(false);
    localStorage.setItem("theme", "dark");
    expect(markInitialColorScheme()).toBe("dark");
    expect(document.documentElement.style.getPropertyValue("color-scheme")).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    localStorage.removeItem("theme");
  });

  it("follows the OS again once the stored choice is system", () => {
    mockColorScheme(true);
    localStorage.setItem("theme", "system");
    expect(markInitialColorScheme()).toBe("dark");
    localStorage.removeItem("theme");
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
