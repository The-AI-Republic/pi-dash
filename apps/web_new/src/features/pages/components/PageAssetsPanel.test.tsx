// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assets tab (parity/specs/pages.md §6.7, PAGE-048): image rows with a
// thumbnail, name and hover download, the extension renderer for other
// kinds, and the themed prompt with no attachments.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const scheme = vi.hoisted(() => ({ current: "light" as string, customDark: false }));
vi.mock("../../../core/theme/index.js", () => ({
  useResolvedScheme: () => ({ scheme: scheme.current, customDark: scheme.customDark }),
}));

import { PAGE_ASSETS_EMPTY_ART_ALT, PAGE_ASSETS_EMPTY_TITLE, PageAssetsPanel } from "./PageAssetsPanel.js";
import type { PageAssetRow } from "../lib/pagePane.js";

const IMAGE: PageAssetRow = {
  source: "asset-9",
  name: "Sunset",
  kind: "image",
  viewHref: "/api/assets/v2/workspaces/acme/projects/p1/asset-9/",
  downloadHref: "/api/assets/v2/workspaces/acme/projects/p1/download/asset-9/",
  thumbnailHref: "/api/assets/v2/workspaces/acme/projects/p1/asset-9/",
};

describe("PageAssetsPanel", () => {
  it("shows the themed prompt with no attachments", () => {
    render(<PageAssetsPanel assets={[]} />);
    expect(screen.getByText(PAGE_ASSETS_EMPTY_TITLE)).toBeInTheDocument();
    expect(screen.getByAltText(PAGE_ASSETS_EMPTY_ART_ALT)).toBeInTheDocument();
  });

  it("paints the dark illustration for a dark scheme", () => {
    scheme.current = "dark";
    try {
      render(<PageAssetsPanel assets={[]} />);
      expect(screen.getByAltText(PAGE_ASSETS_EMPTY_ART_ALT).getAttribute("src")).toContain("empty-dark.svg");
    } finally {
      scheme.current = "light";
    }
  });

  it("renders an image row with its thumbnail, view and hover download", () => {
    render(<PageAssetsPanel assets={[IMAGE]} />);
    const row = screen.getByTestId("page-asset-row");
    expect(row.getAttribute("href")).toBe(IMAGE.viewHref);
    expect(row.querySelector("p")?.textContent).toBe("Sunset");
    const thumb = row.querySelector<HTMLElement>('div[style*="background-image"]');
    expect(thumb?.style.backgroundImage).toContain(IMAGE.thumbnailHref ?? "");
    const download = row.querySelector('a[target="_blank"]:not([data-testid])');
    expect(download?.getAttribute("href")).toBe(IMAGE.downloadHref);
    // Hidden until the row is hovered (or the action is focused).
    expect(download?.className).toContain("opacity-0");
    expect(download?.className).toContain("group-hover/asset:opacity-100");
  });

  it("keeps the download action from activating the row's view link", () => {
    render(<PageAssetsPanel assets={[IMAGE]} />);
    const row = screen.getByTestId("page-asset-row");
    const download = row.querySelector<HTMLAnchorElement>('a[target="_blank"][data-asset-download]');
    if (download === null) throw new Error("no download action");
    const rowClick = vi.fn();
    row.addEventListener("click", rowClick);
    const downloadClick = fireEvent.click(download);
    expect(rowClick).toHaveBeenCalledTimes(1);
    expect(downloadClick).toBe(true);
  });

  it("leaves the thumbnail out of a row with no picture", () => {
    render(<PageAssetsPanel assets={[{ ...IMAGE, thumbnailHref: null }]} />);
    expect(screen.getByTestId("page-asset-row").querySelector('div[style*="background-image"]')).toBeNull();
  });

  it("hands other kinds to the extension renderer", () => {
    const renderAttachment = vi.fn((asset: PageAssetRow) => <span>licensed {asset.name}</span>);
    render(
      <PageAssetsPanel
        assets={[{ ...IMAGE, kind: "attachment", thumbnailHref: null, name: "brief.pdf" }]}
        renderAttachment={renderAttachment}
      />
    );
    expect(renderAttachment).toHaveBeenCalledTimes(1);
    expect(screen.getByText("licensed brief.pdf")).toBeInTheDocument();
    expect(screen.getByText("brief.pdf")).toBeInTheDocument();
    expect(screen.queryByTestId("page-asset-row")).not.toBeInTheDocument();
  });

  it("shows only the name when no extension claims the kind", () => {
    render(<PageAssetsPanel assets={[{ ...IMAGE, kind: "attachment", thumbnailHref: null, name: "brief.pdf" }]} />);
    expect(screen.getByText("brief.pdf")).toBeInTheDocument();
  });
});
