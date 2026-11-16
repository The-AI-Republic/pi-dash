// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Marketing layout (CLOUD-003/004/005): the edition gate, the crawler head
// and the lazy body. The layout component is module-local (the router only
// splits a local component out of the entry chunk), so the test reads it
// from the route options and renders the lazy surface with a resolved
// chunk.
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const locationHarness = vi.hoisted(() => ({ pathname: "/home" }));

vi.mock("@tanstack/react-router", async () => {
  const actual = await vi.importActual<typeof import("@tanstack/react-router")>("@tanstack/react-router");
  return {
    ...actual,
    useLocation: () => ({ pathname: locationHarness.pathname }),
    createFileRoute: () => (options: unknown) => ({ options }),
  };
});

import { edition } from "../core/edition/index.js";
import { Route } from "./_marketing.js";

const MarketingLayout = Route.options.component as () => React.ReactElement;

afterEach(() => {
  delete edition.routes;
  delete edition.slots;
  locationHarness.pathname = "/home";
  document.head.innerHTML = "<title>served</title>";
});

function declareMarketing(): void {
  edition.routes = [
    { path: "/", scope: "top-level" },
    { path: "home", scope: "top-level" },
    { path: "docs", scope: "top-level" },
    { path: "downloads", scope: "top-level" },
    { path: "pricing", scope: "top-level" },
  ];
}

describe("MarketingLayout", () => {
  it("wraps a declared page in the frame and mounts the page body", async () => {
    declareMarketing();
    render(<MarketingLayout />);
    expect(await screen.findByRole("banner")).toBeTruthy();
    expect(screen.getByRole("contentinfo")).toBeTruthy();
    expect(await screen.findByRole("heading", { level: 1 })).toBeTruthy();
  });

  it("serves crawlable metadata for the current page", async () => {
    declareMarketing();
    render(<MarketingLayout />);
    await waitFor(() => {
      expect(document.querySelector('meta[name="robots"]')?.getAttribute("content")).toBe("index, follow");
    });
    expect(document.querySelector('link[rel="canonical"]')?.getAttribute("href")).toBe(
      "https://pidash.airepublic.com/home"
    );
    expect(document.querySelector('meta[property="og:url"]')?.getAttribute("content")).toBe(
      "https://pidash.airepublic.com/home"
    );
  });

  it("normalizes a trailing slash before choosing the page metadata", async () => {
    declareMarketing();
    locationHarness.pathname = "/pricing/";
    render(<MarketingLayout />);
    await waitFor(() => {
      expect(document.querySelector('link[rel="canonical"]')?.getAttribute("href")).toBe(
        "https://pidash.airepublic.com/pricing"
      );
    });
  });

  it("renders not-found when the edition does not declare the address", () => {
    locationHarness.pathname = "/home";
    render(<MarketingLayout />);
    expect(screen.getByRole("heading", { name: "That page does not exist" })).toBeTruthy();
    expect(screen.queryByRole("banner")).toBeNull();
  });

  it("renders not-found for a path outside the marketing set even when declared", () => {
    declareMarketing();
    locationHarness.pathname = "/somewhere";
    render(<MarketingLayout />);
    expect(screen.getByRole("heading", { name: "That page does not exist" })).toBeTruthy();
  });
});
