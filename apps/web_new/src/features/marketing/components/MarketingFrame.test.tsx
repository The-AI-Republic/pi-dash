// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared frame (CLOUD-004). The oracle reads every anchor with its href,
// label, target and visibility, so these assertions mirror that read.
import { render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { FOOTER_LINKS, HEADER_LINKS, REPOSITORY_URL, TERMS_URL, PRIVACY_URL } from "../content.js";
import { MarketingFrame } from "./MarketingFrame.js";

function header(): HTMLElement {
  return screen.getByRole("banner");
}

function footer(): HTMLElement {
  return screen.getByRole("contentinfo");
}

function facts(scope: HTMLElement) {
  return [...scope.querySelectorAll("a")].map((anchor) => ({
    label: (anchor.getAttribute("aria-label") ?? anchor.textContent ?? "").replace(/\s+/g, " ").trim(),
    href: anchor.getAttribute("href"),
    external: anchor.getAttribute("target") === "_blank",
  }));
}

describe("MarketingFrame", () => {
  it("renders the header links in the oracle order with one external repository link", () => {
    render(
      <MarketingFrame>
        <p>page</p>
      </MarketingFrame>
    );
    expect(facts(header()).map((link) => link.href)).toEqual([
      "/home",
      "/downloads",
      "/docs",
      "/pricing",
      REPOSITORY_URL,
      "/login",
      "/login",
    ]);
    expect(
      facts(header())
        .filter((link) => link.external)
        .map((link) => link.href)
    ).toEqual([REPOSITORY_URL]);
    expect(facts(header())[0]?.label).toBe("Pi Dash home");
  });

  it("renders the footer product and legal links, leaving legal links in place", () => {
    render(
      <MarketingFrame>
        <p>page</p>
      </MarketingFrame>
    );
    expect(facts(footer()).map((link) => link.href)).toEqual([
      "/downloads",
      "/docs",
      "/pricing",
      REPOSITORY_URL,
      TERMS_URL,
      PRIVACY_URL,
    ]);
    expect(
      facts(footer())
        .filter((link) => link.external)
        .map((link) => link.href)
    ).toEqual([REPOSITORY_URL]);
  });

  it("hides the secondary header links responsively instead of dropping them", () => {
    render(
      <MarketingFrame>
        <p>page</p>
      </MarketingFrame>
    );
    const byLabel = new Map(
      [...header().querySelectorAll("a")].map((anchor) => [
        (anchor.getAttribute("aria-label") ?? anchor.textContent ?? "").trim(),
        anchor,
      ])
    );
    expect(byLabel.get("Downloads")?.className).toContain("inline-flex");
    expect(byLabel.get("Sign in")?.className).toContain("inline-flex");
    expect(byLabel.get("Docs")?.className).toContain("hidden sm:inline-flex");
    expect(byLabel.get("Pricing")?.className).toContain("hidden sm:inline-flex");
    expect(byLabel.get("Start free")?.className).toContain("hidden sm:inline-flex");
    expect(byLabel.get("GitHub")?.className).toContain("hidden md:inline-flex");
  });

  it("gives every header and footer link a visible focus ring", () => {
    render(
      <MarketingFrame>
        <p>page</p>
      </MarketingFrame>
    );
    for (const anchor of [...header().querySelectorAll("a"), ...footer().querySelectorAll("a")]) {
      expect(anchor.className).toContain("focus-visible:ring-2");
      expect(anchor.className).toContain("focus-visible:outline-none");
    }
  });

  it("puts the content in the page's main landmark under one header and footer", () => {
    render(
      <MarketingFrame>
        <h1>Page heading</h1>
      </MarketingFrame>
    );
    const main = screen.getByRole("main");
    expect(within(main).getByRole("heading", { level: 1, name: "Page heading" })).toBeTruthy();
    expect(screen.getAllByRole("banner")).toHaveLength(1);
    expect(screen.getAllByRole("contentinfo")).toHaveLength(1);
  });

  it("keeps the decorative backdrop out of the accessibility tree", () => {
    const { container } = render(
      <MarketingFrame>
        <p>page</p>
      </MarketingFrame>
    );
    const backdrop = container.querySelector("[data-marketing-backdrop]");
    expect(backdrop).not.toBeNull();
    expect(backdrop?.getAttribute("aria-hidden")).toBe("true");
    expect(backdrop?.querySelector("a, button, input")).toBeNull();
  });

  it("paints the decorative backdrop above the frame background, under the content", () => {
    const { container } = render(
      <MarketingFrame>
        <p>page</p>
      </MarketingFrame>
    );
    const frame = container.firstElementChild as HTMLElement;
    const backdrop = container.querySelector("[data-marketing-backdrop]") as HTMLElement;
    const backdropClass = backdrop.getAttribute("class") ?? "";
    // Isolation keeps the backdrop's z-index scoped to this frame, and the
    // content layer above it (z-10) leaves the illustration visible.
    expect(frame.className).toContain("isolate");
    expect(backdropClass).toContain("z-0");
    // Fixed, like the old page: the map stays put while the content scrolls.
    expect(backdropClass).toContain("fixed");
    expect(backdropClass).not.toContain("-z-");
    for (const layer of [screen.getByRole("banner"), screen.getByRole("main"), screen.getByRole("contentinfo")]) {
      expect(layer.className).toContain("z-10");
    }
    // The tint is the page's text colour behind the map mask, so it reads
    // in both light and dark themes instead of painting black on black.
    expect(backdropClass).toContain("bg-(--text)");
  });

  it("keeps the exported link sets in the order the frame renders", () => {
    expect(HEADER_LINKS.map((link) => link.label)).toEqual([
      "Downloads",
      "Docs",
      "Pricing",
      "GitHub",
      "Sign in",
      "Start free",
    ]);
    expect(FOOTER_LINKS.map((link) => link.label)).toEqual([
      "Downloads",
      "Docs",
      "Pricing",
      "GitHub",
      "Terms",
      "Privacy",
    ]);
  });
});
