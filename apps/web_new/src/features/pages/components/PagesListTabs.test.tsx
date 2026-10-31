// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// PAGE-002 tab labels, shareable hrefs and the active marker.

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, ...rest }: { to: string; children: React.ReactNode }) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
}));

import { PAGES_PROJECT_ID } from "../testing.js";
import { PagesListTabs } from "./PagesListTabs.js";

describe("PagesListTabs", () => {
  it("renders all three shareable links and marks the active one", () => {
    render(<PagesListTabs workspaceSlug="acme" projectId={PAGES_PROJECT_ID} activeTab="private" />);
    expect(screen.getByRole("link", { name: "Public" })).toHaveAttribute(
      "href",
      `/acme/projects/${PAGES_PROJECT_ID}/pages?type=public`
    );
    expect(screen.getByRole("link", { name: "Private" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: "Archived" })).toHaveAttribute(
      "href",
      `/acme/projects/${PAGES_PROJECT_ID}/pages?type=archived`
    );
  });
});
