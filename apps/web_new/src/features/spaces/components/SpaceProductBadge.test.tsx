// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const embed = vi.hoisted(() => ({ framed: false }));
vi.mock("../lib/spaceEmbed.js", () => ({ useSpaceEmbedded: () => embed.framed }));

import { DEFAULT_SPACE_BADGE_URL, resolveSpaceBadgeUrl, SpaceProductBadge } from "./SpaceProductBadge.js";

afterEach(() => {
  embed.framed = false;
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("resolveSpaceBadgeUrl", () => {
  it("defaults to the product site without configuration", () => {
    expect(resolveSpaceBadgeUrl(undefined)).toBe(DEFAULT_SPACE_BADGE_URL);
    expect(resolveSpaceBadgeUrl(42)).toBe(DEFAULT_SPACE_BADGE_URL);
  });

  it("honors a configured site address", () => {
    expect(resolveSpaceBadgeUrl("  https://example.test/shop/  ")).toBe("https://example.test/shop/");
  });

  it("treats a blank address as absent", () => {
    expect(resolveSpaceBadgeUrl("   ")).toBeNull();
  });
});

describe("SpaceProductBadge", () => {
  it("pins a new-tab link to the product site in the corner", () => {
    vi.stubEnv("VITE_WEBSITE_URL", undefined as unknown as string);
    render(<SpaceProductBadge />);
    const badge = screen.getByTestId("space-product-badge");
    expect(badge).toHaveAttribute("href", "https://airepublic.com");
    expect(badge).toHaveAttribute("target", "_blank");
    expect(badge.getAttribute("rel")).toContain("noopener");
    expect(badge).toHaveTextContent("Built with");
    expect(badge).toHaveTextContent("Pi Dash Publish");
    expect(badge.className).toContain("fixed");
  });

  it("follows a configured site address", () => {
    vi.stubEnv("VITE_WEBSITE_URL", "https://example.test/");
    render(<SpaceProductBadge />);
    expect(screen.getByTestId("space-product-badge")).toHaveAttribute("href", "https://example.test/");
  });

  it("renders nothing when blanked, disabled or framed", () => {
    vi.stubEnv("VITE_WEBSITE_URL", "   ");
    const { unmount } = render(<SpaceProductBadge />);
    expect(screen.queryByTestId("space-product-badge")).not.toBeInTheDocument();
    unmount();

    vi.stubEnv("VITE_WEBSITE_URL", "https://example.test/");
    const enabled = render(<SpaceProductBadge disabled />);
    expect(screen.queryByTestId("space-product-badge")).not.toBeInTheDocument();
    enabled.unmount();

    embed.framed = true;
    render(<SpaceProductBadge />);
    expect(screen.queryByTestId("space-product-badge")).not.toBeInTheDocument();
  });
});
