// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Not-found panel (parity/specs/pages.md §4.2, PAGE-023): the pinned
// heading, permission copy and the one recovery link back to the list.

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

import { PageDetailPanel, PAGE_NOT_FOUND_BACK, PAGE_NOT_FOUND_BODY, PAGE_NOT_FOUND_TITLE } from "./PageDetailPanel.js";

describe("PageDetailPanel", () => {
  it("explains the refusal and points back at the collection", () => {
    render(<PageDetailPanel workspaceSlug="acme" projectId="project-1" />);
    expect(screen.getByRole("heading", { name: PAGE_NOT_FOUND_TITLE })).toBeInTheDocument();
    expect(screen.getByText(PAGE_NOT_FOUND_BODY)).toBeInTheDocument();
    expect(PAGE_NOT_FOUND_BODY).toContain("permission");
    const back = screen.getByRole("link", { name: PAGE_NOT_FOUND_BACK });
    expect(back).toHaveAttribute("href", "/acme/projects/project-1/pages/");
  });

  it("mounts no editable region", () => {
    const { container } = render(<PageDetailPanel workspaceSlug="acme" projectId="project-1" />);
    expect(container.querySelector('[contenteditable="true"]')).toBeNull();
  });
});
