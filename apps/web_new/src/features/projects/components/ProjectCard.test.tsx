// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// Stand-in for the router Link: navigation is TanStack's own tested
// behavior. What this suite owns is the card content plus link target.
vi.mock("@tanstack/react-router", () => ({
  Link: (props: {
    params?: Record<string, string>;
    children?: React.ReactNode;
    className?: string;
    ["data-testid"]?: string;
  }) => (
    <a
      href={`/acme/projects/${props.params?.["projectId"] ?? ""}/issues`}
      className={props.className}
      data-testid={props["data-testid"]}
    >
      {props.children}
    </a>
  ),
}));

import { ProjectCard } from "./ProjectCard.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

describe("ProjectCard", () => {
  it("links the project name into its work-items view", () => {
    render(<ProjectCard workspaceSlug="acme" project={{ id: PROJECT_ID, name: "Website relaunch" }} />);

    const link = screen.getByRole("link", { name: "Website relaunch" });
    expect(link).toHaveAttribute("href", `/acme/projects/${PROJECT_ID}/issues`);
    expect(link).toHaveAttribute("data-testid", "project-card");
    expect(screen.getByRole("heading", { name: "Website relaunch" })).not.toBeNull();
  });
});
