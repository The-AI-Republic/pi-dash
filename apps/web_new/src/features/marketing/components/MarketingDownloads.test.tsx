// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Downloads page (CLOUD-007/008). Card order, hrefs, the notice and the
// CLI/web destinations are what the oracle pins; copy is free.
import { render } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { CLI_MSI_URL, DESKTOP_RELEASE_BASE } from "../content.js";
import { MarketingDownloads } from "./MarketingDownloads.js";

describe("MarketingDownloads", () => {
  it("owns the page's only top-level heading", () => {
    const { container } = render(<MarketingDownloads />);
    expect(container.querySelectorAll("h1")).toHaveLength(1);
  });

  it("offers one installer card per OS in oracle order with arch notes", () => {
    const { container } = render(<MarketingDownloads />);
    const cards = [...container.querySelectorAll("article")];
    expect(cards).toHaveLength(3);
    expect(cards.map((card) => card.querySelector("h3")?.textContent)).toEqual(["macOS", "Windows", "Linux"]);
    for (const card of cards) {
      expect(card.querySelector("p")?.textContent?.length).toBeGreaterThan(0);
    }
  });

  it("resolves every download through the stable latest-release address", () => {
    const { container } = render(<MarketingDownloads />);
    const cards = [...container.querySelectorAll("article")];
    const hrefs = cards.map((card) => [...card.querySelectorAll("a")].map((anchor) => anchor.getAttribute("href")));
    expect(hrefs).toEqual([
      [`${DESKTOP_RELEASE_BASE}/pidash-macos-universal.dmg`],
      [`${DESKTOP_RELEASE_BASE}/pidash-windows-x64-setup.exe`, `${DESKTOP_RELEASE_BASE}/pidash-windows-x64.msi`],
      [`${DESKTOP_RELEASE_BASE}/pidash-linux-x64.AppImage`, `${DESKTOP_RELEASE_BASE}/pidash-linux-x64.deb`],
    ]);
  });

  it("warns the installers are not yet publisher-signed", () => {
    const { container } = render(<MarketingDownloads />);
    expect(container.querySelector('[role="note"]')?.textContent).toContain("not yet publisher-signed");
  });

  it("covers the standalone CLI installer with the sign-in step and the docs link", () => {
    const { container } = render(<MarketingDownloads />);
    const cli = container.querySelector("#cli-heading")?.closest("section");
    expect(cli?.tagName).toBe("SECTION");
    expect(cli?.querySelector('a[href$=".msi"]')?.getAttribute("href")).toBe(CLI_MSI_URL);
    expect(cli?.textContent).toContain("pidash auth login");
    expect(cli?.querySelector('a[href="/docs"]')?.getAttribute("href")).toBe("/docs");
  });

  it("points browser-preferring visitors at web sign-in from the closing section", () => {
    const { container } = render(<MarketingDownloads />);
    const all = [...container.querySelectorAll("section")];
    expect(all[all.length - 1]!.querySelector('a[href="/login"]')?.getAttribute("href")).toBe("/login");
  });
});
