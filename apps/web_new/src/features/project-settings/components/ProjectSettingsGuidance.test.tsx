// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bare-path guidance (PSET-002): the member-of-nothing view offers the
// creation entry.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  NO_PROJECTS_BODY,
  NO_PROJECTS_HEADING,
  PROJECTS_GUIDE_LABEL,
  ProjectSettingsGuidance,
  START_FIRST_PROJECT_LABEL,
} from "./ProjectSettingsGuidance.js";
import { DOCS_URL } from "../../../shared/shell/externalLinks.js";

describe("ProjectSettingsGuidance", () => {
  it("explains the area and opens creation", () => {
    const onCreate = vi.fn();
    render(<ProjectSettingsGuidance onCreate={onCreate} />);
    expect(screen.getByTestId("project-settings-guidance")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: NO_PROJECTS_HEADING })).toBeInTheDocument();
    expect(screen.getByText(NO_PROJECTS_BODY)).toBeInTheDocument();
    const guide = screen.getByRole("link", { name: PROJECTS_GUIDE_LABEL });
    expect(guide).toHaveAttribute("href", DOCS_URL);
    expect(guide).toHaveAttribute("target", "_blank");
    fireEvent.click(screen.getByRole("button", { name: START_FIRST_PROJECT_LABEL }));
    expect(onCreate).toHaveBeenCalledTimes(1);
  });
});
