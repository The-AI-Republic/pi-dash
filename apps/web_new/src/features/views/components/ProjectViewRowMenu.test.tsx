// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { ProjectViewMenuItemId } from "../lib/projectViewActions.js";
import { ProjectViewRowMenu } from "./ProjectViewRowMenu.js";

describe("ProjectViewRowMenu", () => {
  it("names its trigger after the row it acts on", () => {
    render(<ProjectViewRowMenu viewName="Triage" items={["open", "copy"]} onSelect={() => {}} />);
    expect(screen.getByRole("button", { name: "Actions for Triage" })).toBeInTheDocument();
  });

  it("renders the permitted entries in order, each with an icon", async () => {
    render(
      <ProjectViewRowMenu viewName="Triage" items={["edit", "open", "copy", "publish", "delete"]} onSelect={() => {}} />
    );
    fireEvent.click(screen.getByRole("button", { name: "Actions for Triage" }));
    await waitFor(() => expect(screen.getAllByRole("menuitem")).toHaveLength(5));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Edit",
      "Open in new tab",
      "Copy link",
      "Publish",
      "Delete",
    ]);
    for (const item of screen.getAllByRole("menuitem")) {
      expect(item.querySelector("svg")).not.toBeNull();
    }
  });

  it("reports each pick", async () => {
    const onSelect = vi.fn();
    const items: ProjectViewMenuItemId[] = ["edit", "open", "copy", "publish", "delete"];
    const labels = ["Edit", "Open in new tab", "Copy link", "Publish", "Delete"];
    render(<ProjectViewRowMenu viewName="Triage" items={items} onSelect={onSelect} />);
    for (const [index, label] of labels.entries()) {
      fireEvent.click(screen.getByRole("button", { name: "Actions for Triage" }));
      fireEvent.click(await screen.findByRole("menuitem", { name: label }));
      expect(onSelect).toHaveBeenNthCalledWith(index + 1, items[index]);
    }
  });
});
