// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General tab body mount (PSET-001): names the project, edits locally
// and starts blank when the context has not resolved.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { ProjectSettingsGeneralMount } from "./ProjectSettingsGeneralMount.js";
import { ProjectSettingsContext, type ProjectSettingsContextValue } from "./ProjectSettingsLayout.js";

function renderMount(project: ProjectSettingsContextValue["project"]): void {
  const value: ProjectSettingsContextValue = {
    workspaceSlug: "acme",
    projectId: "223e4567-e89b-12d3-a456-426614174001",
    role: 20,
    workspaceAdmin: true,
    project,
  };
  render(
    <ProjectSettingsContext.Provider value={value}>
      <ProjectSettingsGeneralMount />
    </ProjectSettingsContext.Provider>
  );
}

describe("ProjectSettingsGeneralMount", () => {
  it("prefills and edits the identity fields locally", () => {
    renderMount({ name: "Website relaunch", identifier: "WEB", logoProps: {} });
    const name = screen.getByRole("textbox", { name: "Project name" });
    const identifier = screen.getByRole("textbox", { name: "Short code" });
    expect(name).toHaveValue("Website relaunch");
    expect(identifier).toHaveValue("WEB");
    fireEvent.change(name, { target: { value: "Renamed" } });
    fireEvent.change(identifier, { target: { value: "RN" } });
    expect(name).toHaveValue("Renamed");
    expect(identifier).toHaveValue("RN");
    expect(screen.getByTestId("project-settings-general")).toHaveAttribute("data-slice", "NEWFRONT-572");
  });

  it("starts blank without a resolved project", () => {
    renderMount(null);
    expect(screen.getByRole("textbox", { name: "Project name" })).toHaveValue("");
    expect(screen.getByRole("textbox", { name: "Short code" })).toHaveValue("");
  });
});
