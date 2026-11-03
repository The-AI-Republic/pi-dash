// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { Folder } from "lucide-react";
import * as React from "react";
import { describe, expect, it } from "vitest";
import { Breadcrumbs, BREADCRUMB_LONG_LABEL_LENGTH } from "./Breadcrumbs";

describe("Breadcrumbs", () => {
  it("renders nothing for an empty trail", () => {
    const { container } = render(<Breadcrumbs entries={[]} />);
    expect(container.firstChild).toBeNull();
  });

  it("links every entry with a target except the terminal crumb", () => {
    render(
      <Breadcrumbs
        entries={[
          { label: "Projects", href: "/acme/projects" },
          { label: "Archived", href: "/acme/projects/archives" },
        ]}
      />
    );
    expect(screen.getByRole("link", { name: "Projects" })).toHaveAttribute("href", "/acme/projects");
    const terminal = screen.getByText("Archived");
    expect(terminal.tagName).toBe("SPAN");
    expect(terminal.parentElement).toHaveAttribute("aria-current", "page");
    expect(screen.queryByRole("link", { name: "Archived" })).toBeNull();
  });

  it("renders a single entry as terminal text", () => {
    render(<Breadcrumbs entries={[{ label: "Projects", href: "/acme/projects" }]} />);
    expect(screen.queryByRole("link")).toBeNull();
    expect(screen.getByText("Projects").parentElement).toHaveAttribute("aria-current", "page");
  });

  it("links the terminal entry when the trail opts into a self-referencing crumb", () => {
    render(
      <Breadcrumbs
        terminalAsLink
        entries={[{ label: "Apollo" }, { label: "Intake", href: "/acme/projects/p-1/intake" }]}
      />
    );
    const terminal = screen.getByRole("link", { name: "Intake" });
    expect(terminal).toHaveAttribute("href", "/acme/projects/p-1/intake");
    expect(terminal).toHaveAttribute("aria-current", "page");
    expect(terminal).toHaveTextContent("Intake");
    expect(screen.getByText("Apollo").tagName).toBe("SPAN");
  });

  it("keeps the terminal entry plain text when the opt-in crumb has no target", () => {
    render(<Breadcrumbs terminalAsLink entries={[{ label: "Apollo" }, { label: "Intake" }]} />);
    expect(screen.queryByRole("link")).toBeNull();
    expect(screen.getByText("Intake").parentElement).toHaveAttribute("aria-current", "page");
  });

  it("renders entries without a target as plain text", () => {
    render(<Breadcrumbs entries={[{ label: "Projects" }, { label: "Archived" }]} />);
    expect(screen.queryByRole("link")).toBeNull();
    expect(screen.getByText("Projects")).not.toHaveAttribute("aria-current");
  });

  it("keeps the full text on hover for long labels only", () => {
    const long = `a label longer than ${BREADCRUMB_LONG_LABEL_LENGTH} characters`;
    render(<Breadcrumbs entries={[{ label: "Projects", href: "/acme/projects" }, { label: long }]} />);
    expect(screen.getByText(long)).toHaveAttribute("title", long);
    expect(screen.getByText("Projects")).not.toHaveAttribute("title");
  });

  it("renders leading icons decoratively", () => {
    render(
      <Breadcrumbs
        entries={[
          { label: "Projects", href: "/acme/projects", icon: <Folder data-testid="crumb-icon" size={14} /> },
          { label: "Archived" },
        ]}
      />
    );
    const icon = screen.getByTestId("crumb-icon");
    expect(icon.closest('[aria-hidden="true"]')).not.toBeNull();
  });

  it("names the landmark and separates crumbs", () => {
    const { container } = render(
      <Breadcrumbs
        label="Project trail"
        entries={[{ label: "Projects", href: "/acme/projects" }, { label: "Archived" }]}
      />
    );
    expect(screen.getByRole("navigation", { name: "Project trail" })).not.toBeNull();
    expect(container.querySelectorAll("li").length).toBe(2);
  });
});
