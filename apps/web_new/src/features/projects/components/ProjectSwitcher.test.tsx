// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ProjectSwitcher } from "./ProjectSwitcher.js";

const PROJECTS = [
  { id: "p1", name: "Web" },
  { id: "p2", name: "Mobile" },
];

function setup(onSelectProject: (id: string) => void = vi.fn()) {
  render(<ProjectSwitcher current={PROJECTS[0]!} joined={PROJECTS} onSelectProject={onSelectProject} />);
}

describe("ProjectSwitcher", () => {
  it("names the current project on a truncating line", () => {
    setup();
    expect(screen.getByTestId("project-switcher-trigger")).toHaveTextContent("Web");
    expect(screen.getByTestId("project-switcher-name")).toHaveTextContent("Web");
  });

  it("reveals the full name on hover", async () => {
    const user = userEvent.setup();
    setup();
    await user.hover(screen.getByTestId("project-switcher-trigger"));
    expect(await screen.findByRole("tooltip", { name: "Web" })).toBeInTheDocument();
  });

  it("lists joined projects with icons and selects through the callback", () => {
    const onSelectProject = vi.fn();
    setup(onSelectProject);
    fireEvent.click(screen.getByTestId("project-switcher-trigger"));
    expect(screen.getByRole("menuitem", { name: "Web" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Mobile" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "Mobile" }));
    expect(onSelectProject).toHaveBeenCalledWith("p2");
  });
});
