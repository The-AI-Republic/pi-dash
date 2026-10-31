// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page chrome (PSET-001): the header names the area and tab with the
// project emblem; the mount point names the owning slice.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ProjectSettingsContext, type ProjectSettingsContextValue } from "./ProjectSettingsLayout.js";
import { ProjectSettingsPage } from "./ProjectSettingsPage.js";
import { ProjectSettingsTabMount } from "./ProjectSettingsTabMount.js";

const BASE: ProjectSettingsContextValue = {
  workspaceSlug: "acme",
  projectId: "223e4567-e89b-12d3-a456-426614174001",
  role: 20,
  workspaceAdmin: true,
  project: { name: "Website relaunch", identifier: "WEB", logoProps: {} },
};

function wrap(value: ProjectSettingsContextValue, children: React.ReactNode): React.ReactElement {
  return <ProjectSettingsContext.Provider value={value}>{children}</ProjectSettingsContext.Provider>;
}

describe("ProjectSettingsPage", () => {
  beforeEach(() => createAppClient("https://api.test"));

  afterEach(() => resetAppClient());

  it("titles the tab and carries the project emblem", () => {
    render(wrap(BASE, <ProjectSettingsPage title="Members">body</ProjectSettingsPage>));
    expect(screen.getByRole("heading", { level: 1, name: "Members" })).toBeInTheDocument();
    expect(screen.getByText("Project settings")).toBeInTheDocument();
    expect(screen.getByTestId("project-logo")).toHaveTextContent("W");
    expect(screen.getByText("body")).toBeInTheDocument();
  });

  it("renders the header without an emblem before the shell resolves", () => {
    render(wrap({ ...BASE, project: null }, <ProjectSettingsPage title="States">body</ProjectSettingsPage>));
    expect(screen.queryByTestId("project-logo")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: "States" })).toBeInTheDocument();
  });

  it("renders a saved emoji mark in the header", () => {
    render(
      wrap(
        { ...BASE, project: { ...BASE.project!, logoProps: { in_use: "emoji", emoji: { value: "128293" } } } },
        <ProjectSettingsPage title="General">body</ProjectSettingsPage>
      )
    );
    expect(screen.getByTestId("project-logo")).toHaveTextContent("🔥");
  });

  it("renders a saved icon mark in the header", () => {
    render(
      wrap(
        {
          ...BASE,
          project: {
            ...BASE.project!,
            logoProps: { in_use: "icon", icon: { name: "rocket_launch", color: "#f97316" } },
          },
        },
        <ProjectSettingsPage title="General">body</ProjectSettingsPage>
      )
    );
    expect(screen.getByTestId("project-logo")).toHaveAttribute("data-mark", "icon");
  });
});

describe("ProjectSettingsTabMount", () => {
  it("names the tab and its owning slice without a second page header", () => {
    render(wrap(BASE, <ProjectSettingsTabMount title="Members" owner="NEWFRONT-999" />));
    expect(screen.queryByRole("heading")).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Members settings" })).toHaveAttribute("data-slice", "NEWFRONT-999");
    expect(screen.getByText("This tab’s screen arrives with its own slice.")).toBeInTheDocument();
  });
});
